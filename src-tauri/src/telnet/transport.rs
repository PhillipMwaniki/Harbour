//! A telnet connection, seen as a [`Transport`].
//!
//! Like the SSH transport, two tasks drive it and nothing here blocks: `write`,
//! `resize` and `kill` all queue a command. The reader turns the socket into
//! the same `Receiver<Vec<u8>>` a pty produces - after the telnet negotiation
//! has been stripped out of it - so the batching pump, the ack budget and the
//! terminal are shared with every other kind of session.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Arc;

use parking_lot::Mutex;
use tokio::io::{AsyncReadExt, AsyncWriteExt};
use tokio::net::tcp::OwnedWriteHalf;
use tokio::net::TcpStream;
use tokio::sync::mpsc;

use crate::error::{AppError, AppResult};
use crate::session::{ExitReason, Transport};
use crate::telnet::{escape_input, naws_subneg, Parser};

/// Matches the SSH transport, so both behave alike under load; the real
/// throttle is the ack budget in `session::reader`.
const READ_QUEUE_DEPTH: usize = 256;

enum Command {
    /// Raw bytes for the socket - user input already escaped, or a
    /// negotiation reply formed by the parser.
    Data(Vec<u8>),
    Close,
}

pub struct TelnetTransport {
    commands: mpsc::UnboundedSender<Command>,
    /// The last size the terminal reported, sent as NAWS once the server has
    /// agreed to it.
    size: Arc<Mutex<(u16, u16)>>,
    naws: Arc<AtomicBool>,
    /// Set by `kill`, read by the reader task, so a session the user closed and
    /// a connection that dropped are told apart.
    closing: Arc<AtomicBool>,
}

impl TelnetTransport {
    fn send(&self, command: Command) -> AppResult<()> {
        self.commands
            .send(command)
            .map_err(|_| AppError::Telnet("the connection is closed".into()))
    }
}

impl Transport for TelnetTransport {
    fn write(&self, data: &[u8]) -> AppResult<()> {
        self.send(Command::Data(escape_input(data)))
    }

    fn resize(&self, cols: u16, rows: u16) -> AppResult<()> {
        let (cols, rows) = (cols.max(1), rows.max(1));
        *self.size.lock() = (cols, rows);
        // Only worth sending once the server has said it wants window sizes.
        if self.naws.load(Ordering::Acquire) {
            self.send(Command::Data(naws_subneg(cols, rows)))?;
        }
        Ok(())
    }

    fn kill(&self) {
        self.closing.store(true, Ordering::Release);
        let _ = self.send(Command::Close);
    }
}

/// A telnet connection turned into a session's two halves.
pub struct Connected {
    pub transport: TelnetTransport,
    pub output: mpsc::Receiver<Vec<u8>>,
}

/// Opens a telnet connection to `host:port` and starts its reader and writer
/// tasks. `on_exit` fires once, when the connection ends.
pub async fn connect<F>(
    host: &str,
    port: u16,
    cols: u16,
    rows: u16,
    on_exit: F,
) -> AppResult<Connected>
where
    F: FnOnce(ExitReason, Option<u32>) + Send + 'static,
{
    let stream = TcpStream::connect((host, port))
        .await
        .map_err(|err| AppError::Telnet(format!("could not reach {host}:{port}: {err}")))?;
    // Nagle off: an interactive session wants each keystroke on the wire now,
    // not coalesced.
    let _ = stream.set_nodelay(true);
    let (reader, writer) = stream.into_split();

    let (out_tx, out_rx) = mpsc::channel::<Vec<u8>>(READ_QUEUE_DEPTH);
    let (cmd_tx, cmd_rx) = mpsc::unbounded_channel::<Command>();
    let size = Arc::new(Mutex::new((cols.max(1), rows.max(1))));
    let naws = Arc::new(AtomicBool::new(false));
    let closing = Arc::new(AtomicBool::new(false));

    tokio::spawn(write_loop(writer, cmd_rx));
    tokio::spawn(read_loop(
        reader,
        out_tx,
        cmd_tx.clone(),
        Arc::clone(&naws),
        Arc::clone(&size),
        Arc::clone(&closing),
        on_exit,
    ));

    Ok(Connected {
        transport: TelnetTransport {
            commands: cmd_tx,
            size,
            naws,
            closing,
        },
        output: out_rx,
    })
}

async fn write_loop(mut writer: OwnedWriteHalf, mut commands: mpsc::UnboundedReceiver<Command>) {
    while let Some(command) = commands.recv().await {
        match command {
            Command::Data(bytes) => {
                if writer.write_all(&bytes).await.is_err() {
                    break;
                }
                let _ = writer.flush().await;
            }
            Command::Close => break,
        }
    }
    let _ = writer.shutdown().await;
}

#[allow(clippy::too_many_arguments)]
async fn read_loop<F>(
    mut reader: tokio::net::tcp::OwnedReadHalf,
    output: mpsc::Sender<Vec<u8>>,
    commands: mpsc::UnboundedSender<Command>,
    naws: Arc<AtomicBool>,
    size: Arc<Mutex<(u16, u16)>>,
    closing: Arc<AtomicBool>,
    on_exit: F,
) where
    F: FnOnce(ExitReason, Option<u32>) + Send + 'static,
{
    let mut parser = Parser::new();
    let mut buf = [0u8; 8192];

    let reason = loop {
        match reader.read(&mut buf).await {
            Ok(0) => {
                // A clean close. Whether it was ours or theirs is the only
                // thing the tab needs to tell apart.
                break if closing.load(Ordering::Acquire) {
                    ExitReason::Killed
                } else {
                    ExitReason::Exited
                };
            }
            Ok(n) => {
                let processed = parser.feed(&buf[..n]);
                if !processed.replies.is_empty() {
                    let _ = commands.send(Command::Data(processed.replies));
                }
                // The moment the server agrees to NAWS, send it the size we have.
                if processed.naws_enabled && !naws.swap(true, Ordering::AcqRel) {
                    let (cols, rows) = *size.lock();
                    let _ = commands.send(Command::Data(naws_subneg(cols, rows)));
                }
                if !processed.data.is_empty() && output.send(processed.data).await.is_err() {
                    // The session was dropped; the reader has nowhere to send.
                    break ExitReason::Killed;
                }
            }
            Err(_) => {
                break if closing.load(Ordering::Acquire) {
                    ExitReason::Killed
                } else {
                    ExitReason::Lost
                };
            }
        }
    };

    // Stop the writer too, then report the exit exactly once.
    let _ = commands.send(Command::Close);
    on_exit(reason, None);
}
