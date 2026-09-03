//! Output batching and ack-based backpressure.
//!
//! A pty can produce data far faster than a webview can render it (`cat` of a
//! large file). Two mechanisms keep the UI responsive:
//!
//! 1. **Batching** — chunks are coalesced until [`FLUSH_BYTES`] or
//!    [`FLUSH_INTERVAL`] elapses, so we cross the IPC boundary ~125 times a
//!    second at most instead of once per read.
//! 2. **Backpressure** — every batch handed to the frontend is "charged"
//!    against a budget. The frontend acks bytes once xterm.js has written them.
//!    Above [`MAX_UNACKED_BYTES`] the pump stops pulling, which propagates back
//!    through the channel to the reader thread and finally to the pty itself.

use std::sync::atomic::{AtomicUsize, Ordering};
use std::sync::Arc;
use std::time::Duration;

use tokio::sync::{mpsc, Notify};
use tokio::time::{timeout_at, Instant};

/// Flush as soon as this much output has accumulated.
pub const FLUSH_BYTES: usize = 32 * 1024;
/// ...or this long has passed since the first byte of the batch.
pub const FLUSH_INTERVAL: Duration = Duration::from_millis(8);
/// Stop sending once this many bytes are in flight and unacknowledged.
pub const MAX_UNACKED_BYTES: usize = 1024 * 1024;

/// The frontend is no longer listening, so the pump should stop.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct SinkClosed;

/// Where a pump delivers batched output. Abstracted over the Tauri channel so
/// the batching logic can be tested without a webview.
pub trait OutputSink: Send + 'static {
    fn send(&self, data: Vec<u8>) -> Result<(), SinkClosed>;
}

/// Shared byte budget between the pump and the frontend's acks.
#[derive(Clone, Default)]
pub struct Backpressure {
    unacked: Arc<AtomicUsize>,
    drained: Arc<Notify>,
}

impl Backpressure {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn unacked(&self) -> usize {
        self.unacked.load(Ordering::Acquire)
    }

    fn charge(&self, bytes: usize) {
        self.unacked.fetch_add(bytes, Ordering::AcqRel);
    }

    /// Called from `session_ack` once xterm.js has consumed `bytes`.
    pub fn ack(&self, bytes: usize) {
        // `fetch_update` keeps the counter from wrapping if an ack ever
        // over-reports (e.g. a duplicate ack after a reconnect).
        let _ = self
            .unacked
            .fetch_update(Ordering::AcqRel, Ordering::Acquire, |cur| {
                Some(cur.saturating_sub(bytes))
            });
        self.drained.notify_waiters();
    }

    /// Resolves once the in-flight budget has room again.
    pub async fn wait_for_room(&self) {
        loop {
            // Register interest *before* re-reading the counter, otherwise an
            // ack landing between the check and the await would be lost.
            let notified = self.drained.notified();
            tokio::pin!(notified);
            notified.as_mut().enable();

            if self.unacked() < MAX_UNACKED_BYTES {
                return;
            }
            notified.await;
        }
    }
}

/// Consumes raw pty chunks, coalesces them, and forwards them to `sink`.
///
/// Runs until the producer side closes or the sink goes away.
pub async fn pump<S: OutputSink>(mut rx: mpsc::Receiver<Vec<u8>>, sink: S, bp: Backpressure) {
    loop {
        let Some(first) = rx.recv().await else { return };

        let mut batch = first;
        let deadline = Instant::now() + FLUSH_INTERVAL;
        let mut producer_done = false;

        while batch.len() < FLUSH_BYTES {
            match timeout_at(deadline, rx.recv()).await {
                Ok(Some(chunk)) => batch.extend_from_slice(&chunk),
                Ok(None) => {
                    producer_done = true;
                    break;
                }
                Err(_elapsed) => break,
            }
        }

        bp.wait_for_room().await;
        bp.charge(batch.len());
        if sink.send(batch).is_err() {
            return;
        }
        if producer_done {
            return;
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::Mutex;

    #[derive(Clone, Default)]
    struct VecSink(Arc<Mutex<Vec<Vec<u8>>>>);

    impl OutputSink for VecSink {
        fn send(&self, data: Vec<u8>) -> Result<(), SinkClosed> {
            self.0.lock().unwrap().push(data);
            Ok(())
        }
    }

    #[tokio::test]
    async fn coalesces_small_chunks_into_one_batch() {
        let (tx, rx) = mpsc::channel(64);
        let sink = VecSink::default();
        let bp = Backpressure::new();

        for _ in 0..10 {
            tx.send(b"hello".to_vec()).await.unwrap();
        }
        drop(tx);

        pump(rx, sink.clone(), bp).await;

        let batches = sink.0.lock().unwrap();
        assert_eq!(batches.len(), 1, "10 tiny chunks should flush as one batch");
        assert_eq!(batches[0].len(), 50);
    }

    #[tokio::test]
    async fn flushes_once_the_byte_threshold_is_reached() {
        let (tx, rx) = mpsc::channel(64);
        let sink = VecSink::default();
        let bp = Backpressure::new();

        // A single oversized chunk must flush on its own rather than waiting
        // for more input, so the trailing chunk lands in a second batch.
        tx.send(vec![b'a'; 40 * 1024]).await.unwrap();
        tx.send(vec![b'b'; 16]).await.unwrap();
        drop(tx);

        pump(rx, sink.clone(), bp).await;

        let batches = sink.0.lock().unwrap();
        assert_eq!(batches.len(), 2);
        assert_eq!(batches[0].len(), 40 * 1024);
        assert_eq!(batches[1].len(), 16);
    }

    #[tokio::test]
    async fn charges_and_releases_the_backpressure_budget() {
        let (tx, rx) = mpsc::channel(8);
        let sink = VecSink::default();
        let bp = Backpressure::new();

        tx.send(vec![0u8; 1000]).await.unwrap();
        drop(tx);
        pump(rx, sink, bp.clone()).await;

        assert_eq!(bp.unacked(), 1000);
        bp.ack(1000);
        assert_eq!(bp.unacked(), 0);
    }

    #[tokio::test]
    async fn ack_never_wraps_below_zero() {
        let bp = Backpressure::new();
        bp.ack(10_000);
        assert_eq!(bp.unacked(), 0);
    }

    #[tokio::test]
    async fn pump_stalls_while_the_budget_is_full_and_resumes_on_ack() {
        let (tx, rx) = mpsc::channel(16);
        let sink = VecSink::default();
        let bp = Backpressure::new();

        // First batch fills the budget on its own.
        tx.send(vec![0u8; MAX_UNACKED_BYTES]).await.unwrap();
        tx.send(vec![1u8; 16]).await.unwrap();
        drop(tx);

        let handle = tokio::spawn(pump(rx, sink.clone(), bp.clone()));

        // Give the pump time to deliver batch one and block on batch two.
        tokio::time::sleep(Duration::from_millis(50)).await;
        assert_eq!(
            sink.0.lock().unwrap().len(),
            1,
            "second batch must be held back"
        );

        bp.ack(MAX_UNACKED_BYTES);
        handle.await.unwrap();
        assert_eq!(
            sink.0.lock().unwrap().len(),
            2,
            "ack should release the pump"
        );
    }
}
