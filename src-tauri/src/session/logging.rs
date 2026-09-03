//! Writing a session's output to a file while it runs.
//!
//! Logging sits on the same batches the frontend gets, one level below the
//! IPC boundary, so what lands in the file is exactly what the terminal was
//! sent - not a re-render of what the webview happened to keep in scrollback.
//!
//! The write itself happens on its own thread. A log on a slow or full disk
//! must not stall the output pump: the pump is what feeds backpressure back to
//! the pty, and blocking it to write a log would throttle the session for a
//! reason the user never asked for. If the writer cannot keep up, the log
//! records that it fell behind and the session carries on.

use std::fs::{File, OpenOptions};
use std::io::{BufWriter, Write};
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{mpsc, Arc};

use parking_lot::{Mutex, RwLock};
use serde::{Deserialize, Serialize};

use crate::error::{AppError, AppResult};
use crate::settings::LogFormat;

/// How much output may be waiting to be written before the log gives up on
/// keeping every byte. At 32 KB a batch this is a megabyte or so of slack.
const QUEUE_DEPTH: usize = 32;

/// What the frontend is told about a session's log.
#[derive(Debug, Clone, Default, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogStatus {
    pub active: bool,
    pub path: Option<String>,
    pub format: Option<LogFormat>,
    pub bytes: u64,
    /// Set once the writer has failed; the session keeps running regardless.
    pub error: Option<String>,
}

/// One open log file.
pub struct OutputLog {
    path: PathBuf,
    format: LogFormat,
    tx: Mutex<Option<mpsc::SyncSender<Vec<u8>>>>,
    bytes: Arc<AtomicU64>,
    error: Arc<Mutex<Option<String>>>,
    writer: Mutex<Option<std::thread::JoinHandle<()>>>,
}

impl OutputLog {
    /// Opens `path` and starts the writer thread.
    ///
    /// `append` is what makes reconnecting to the same file safe; without it,
    /// starting a log twice in one evening silently discards the first.
    pub fn start(path: &Path, format: LogFormat, append: bool) -> AppResult<Arc<Self>> {
        if let Some(parent) = path
            .parent()
            .filter(|parent| !parent.as_os_str().is_empty())
        {
            std::fs::create_dir_all(parent).map_err(|err| {
                AppError::LogFailed(format!("could not create {}: {err}", parent.display()))
            })?;
        }

        let file = OpenOptions::new()
            .create(true)
            .write(true)
            .append(append)
            .truncate(!append)
            .open(path)
            .map_err(|err| {
                AppError::LogFailed(format!("could not open {}: {err}", path.display()))
            })?;

        let (tx, rx) = mpsc::sync_channel::<Vec<u8>>(QUEUE_DEPTH);
        let bytes = Arc::new(AtomicU64::new(0));
        let error = Arc::new(Mutex::new(None));

        let thread = std::thread::Builder::new()
            .name("harbour-session-log".into())
            .spawn({
                let bytes = bytes.clone();
                let error = error.clone();
                let display = path.display().to_string();
                move || run(file, rx, format, bytes, error, display)
            })
            .map_err(|err| AppError::LogFailed(format!("could not start the log writer: {err}")))?;

        Ok(Arc::new(Self {
            path: path.to_path_buf(),
            format,
            tx: Mutex::new(Some(tx)),
            bytes,
            error,
            writer: Mutex::new(Some(thread)),
        }))
    }

    /// Queues a batch. Returns immediately, always: see the note at the top of
    /// the file about not stalling the pump.
    pub fn write(&self, data: &[u8]) {
        let guard = self.tx.lock();
        let Some(tx) = guard.as_ref() else { return };
        match tx.try_send(data.to_vec()) {
            Ok(()) => {}
            Err(mpsc::TrySendError::Full(_)) => {
                let mut error = self.error.lock();
                if error.is_none() {
                    *error = Some("the log fell behind the session and dropped output".into());
                }
            }
            Err(mpsc::TrySendError::Disconnected(_)) => {}
        }
    }

    /// Closes the file, waiting for the queued output to be written.
    pub fn stop(&self) {
        // Dropping the sender is what tells the writer to flush and exit.
        self.tx.lock().take();
        if let Some(thread) = self.writer.lock().take() {
            let _ = thread.join();
        }
    }

    pub fn status(&self) -> LogStatus {
        LogStatus {
            active: self.tx.lock().is_some(),
            path: Some(self.path.display().to_string()),
            format: Some(self.format),
            bytes: self.bytes.load(Ordering::Relaxed),
            error: self.error.lock().clone(),
        }
    }
}

impl Drop for OutputLog {
    fn drop(&mut self) {
        self.stop();
    }
}

fn run(
    file: File,
    rx: mpsc::Receiver<Vec<u8>>,
    format: LogFormat,
    bytes: Arc<AtomicU64>,
    error: Arc<Mutex<Option<String>>>,
    path: String,
) {
    let mut out = BufWriter::new(file);
    let mut plain = PlainText::default();
    let mut failed = false;

    for batch in rx {
        if failed {
            continue;
        }
        let data = match format {
            LogFormat::Raw => batch,
            LogFormat::Plain => plain.feed(&batch),
        };
        if data.is_empty() {
            continue;
        }
        match out.write_all(&data) {
            Ok(()) => {
                bytes.fetch_add(data.len() as u64, Ordering::Relaxed);
            }
            Err(err) => {
                tracing::warn!(path = %path, error = %err, "session log write failed");
                *error.lock() = Some(err.to_string());
                failed = true;
            }
        }
    }

    if !failed {
        // A session that ends mid-line still has that line worth keeping.
        let tail = plain.flush();
        if !tail.is_empty() {
            let _ = out.write_all(&tail);
            bytes.fetch_add(tail.len() as u64, Ordering::Relaxed);
        }
    }
    if let Err(err) = out.flush() {
        *error.lock() = Some(err.to_string());
    }
}

/// Turns a terminal stream into something a text editor can read.
///
/// Two things are in the way. Escape sequences, which are removed, and the
/// carriage return, which a progress bar uses to redraw the same line thirty
/// times a second. Keeping every redraw would make the log unreadable, so a
/// `\r` discards the line so far and only the final state of a line is
/// written. A carriage return with nothing written after it is just a line
/// ending, so CRLF survives intact.
#[derive(Default)]
struct PlainText {
    line: Vec<u8>,
    /// A carriage return has been seen and nothing has been written since.
    returned: bool,
    state: EscapeState,
}

#[derive(Default, Clone, Copy, PartialEq, Eq)]
enum EscapeState {
    #[default]
    Text,
    /// Saw ESC; the next byte says what kind of sequence this is.
    Escape,
    /// Inside `ESC [ ... final`.
    Csi,
    /// Inside `ESC ] ... BEL` or `... ESC \`.
    Osc,
    /// Saw ESC inside an OSC string: the next byte may end it.
    OscEscape,
    /// A two-byte sequence whose second byte is consumed unconditionally.
    Skip,
}

impl PlainText {
    fn feed(&mut self, data: &[u8]) -> Vec<u8> {
        let mut out = Vec::with_capacity(data.len());
        for &byte in data {
            match self.state {
                EscapeState::Text => match byte {
                    0x1b => self.state = EscapeState::Escape,
                    b'\r' => self.returned = true,
                    b'\n' => {
                        self.returned = false;
                        out.append(&mut self.line);
                        out.push(b'\n');
                    }
                    // Backspace: shells use it to erase, so honour it.
                    0x08 => {
                        self.line.pop();
                    }
                    // Other C0 controls carry no text; the bell in particular
                    // has no business in a log file.
                    0x00..=0x1f if byte != b'\t' => {}
                    _ => {
                        // The first thing written after a carriage return is
                        // overwriting the line, not continuing it.
                        if self.returned {
                            self.line.clear();
                            self.returned = false;
                        }
                        self.line.push(byte);
                    }
                },
                EscapeState::Escape => {
                    self.state = match byte {
                        b'[' => EscapeState::Csi,
                        b']' => EscapeState::Osc,
                        // ESC P/X/^/_ open string sequences that end the same
                        // way OSC does.
                        b'P' | b'X' | b'^' | b'_' => EscapeState::Osc,
                        b'(' | b')' | b'*' | b'+' | b'#' | b' ' => EscapeState::Skip,
                        _ => EscapeState::Text,
                    };
                }
                EscapeState::Csi => {
                    // Parameter and intermediate bytes, then a final byte.
                    if (0x40..=0x7e).contains(&byte) {
                        self.state = EscapeState::Text;
                    }
                }
                EscapeState::Osc => match byte {
                    0x07 => self.state = EscapeState::Text,
                    0x1b => self.state = EscapeState::OscEscape,
                    _ => {}
                },
                EscapeState::OscEscape => {
                    self.state = if byte == b'\\' {
                        EscapeState::Text
                    } else {
                        EscapeState::Osc
                    };
                }
                EscapeState::Skip => self.state = EscapeState::Text,
            }
        }
        out
    }

    fn flush(&mut self) -> Vec<u8> {
        self.returned = false;
        if self.line.is_empty() {
            return Vec::new();
        }
        let mut out = std::mem::take(&mut self.line);
        out.push(b'\n');
        out
    }
}

/// The log slot on a session, shared between the command handlers that start
/// and stop it and the pump that feeds it.
#[derive(Default)]
pub struct LogSlot(RwLock<Option<Arc<OutputLog>>>);

impl LogSlot {
    pub fn write(&self, data: &[u8]) {
        if let Some(log) = self.0.read().as_ref() {
            log.write(data);
        }
    }

    /// Installs a log, closing whatever was there. Replacing rather than
    /// refusing keeps "log to a different file" a single action.
    pub fn set(&self, log: Arc<OutputLog>) -> LogStatus {
        let status = log.status();
        let previous = self.0.write().replace(log);
        if let Some(previous) = previous {
            previous.stop();
        }
        status
    }

    pub fn clear(&self) -> LogStatus {
        let previous = self.0.write().take();
        match previous {
            Some(log) => {
                log.stop();
                LogStatus {
                    active: false,
                    ..log.status()
                }
            }
            None => LogStatus::default(),
        }
    }

    pub fn status(&self) -> LogStatus {
        match self.0.read().as_ref() {
            Some(log) => log.status(),
            None => LogStatus::default(),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn plain(chunks: &[&[u8]]) -> String {
        let mut text = PlainText::default();
        let mut out = Vec::new();
        for chunk in chunks {
            out.extend(text.feed(chunk));
        }
        out.extend(text.flush());
        String::from_utf8(out).unwrap()
    }

    #[test]
    fn plain_text_keeps_the_text_and_drops_the_colours() {
        assert_eq!(
            plain(&[b"\x1b[31mred\x1b[0m and plain\n"]),
            "red and plain\n"
        );
    }

    #[test]
    fn plain_text_survives_a_sequence_split_across_batches() {
        // The pump splits on byte counts, not on sequence boundaries.
        assert_eq!(plain(&[b"a\x1b[3", b"1mb\n"]), "ab\n");
    }

    #[test]
    fn a_carriage_return_keeps_only_the_final_state_of_the_line() {
        assert_eq!(plain(&[b"10%\r50%\r100%\ndone\n"]), "100%\ndone\n");
        // The usual CRLF is not a redraw and must not eat the line.
        assert_eq!(plain(&[b"one\r\ntwo\r\n"]), "one\ntwo\n");
    }

    #[test]
    fn window_titles_and_bells_do_not_reach_the_file() {
        assert_eq!(plain(&[b"\x1b]0;a title\x07visible\n"]), "visible\n");
        assert_eq!(plain(&[b"\x1b]0;a title\x1b\\visible\n"]), "visible\n");
        assert_eq!(plain(&[b"ding\x07\n"]), "ding\n");
    }

    #[test]
    fn backspace_erases_and_tabs_survive() {
        assert_eq!(plain(&[b"helllo\x08 world\n"]), "helll world\n");
        assert_eq!(plain(&[b"a\tb\n"]), "a\tb\n");
    }

    #[test]
    fn an_unterminated_line_is_still_written() {
        assert_eq!(plain(&[b"no newline here"]), "no newline here\n");
    }

    #[test]
    fn writes_raw_bytes_verbatim() {
        let dir = std::env::temp_dir().join(format!("harbour-log-raw-{}", uuid::Uuid::new_v4()));
        let path = dir.join("session.log");

        let log = OutputLog::start(&path, LogFormat::Raw, false).unwrap();
        log.write(b"\x1b[31mred\x1b[0m\n");
        log.stop();

        assert_eq!(
            std::fs::read(&path).unwrap(),
            b"\x1b[31mred\x1b[0m\n".to_vec()
        );
        assert!(log.status().bytes > 0);
        assert!(!log.status().active);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn plain_logging_writes_readable_text() {
        let dir = std::env::temp_dir().join(format!("harbour-log-plain-{}", uuid::Uuid::new_v4()));
        let path = dir.join("nested").join("session.log");

        let log = OutputLog::start(&path, LogFormat::Plain, false).unwrap();
        log.write(b"\x1b[32m$\x1b[0m ls\r\nfile-a\r\n");
        log.stop();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "$ ls\nfile-a\n");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn appending_keeps_what_was_there() {
        let dir = std::env::temp_dir().join(format!("harbour-log-append-{}", uuid::Uuid::new_v4()));
        let path = dir.join("session.log");

        let first = OutputLog::start(&path, LogFormat::Plain, false).unwrap();
        first.write(b"one\n");
        first.stop();

        let second = OutputLog::start(&path, LogFormat::Plain, true).unwrap();
        second.write(b"two\n");
        second.stop();

        assert_eq!(std::fs::read_to_string(&path).unwrap(), "one\ntwo\n");

        // ...and starting again without append truncates, as asked.
        let third = OutputLog::start(&path, LogFormat::Plain, false).unwrap();
        third.write(b"three\n");
        third.stop();
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "three\n");
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn a_slot_starts_empty_and_reports_what_it_holds() {
        let dir = std::env::temp_dir().join(format!("harbour-log-slot-{}", uuid::Uuid::new_v4()));
        let path = dir.join("session.log");
        let slot = LogSlot::default();

        assert!(!slot.status().active);
        // Writing with no log installed is a no-op, not a panic.
        slot.write(b"dropped on the floor\n");

        slot.set(OutputLog::start(&path, LogFormat::Plain, false).unwrap());
        assert!(slot.status().active);
        slot.write(b"kept\n");

        let stopped = slot.clear();
        assert!(!stopped.active);
        assert_eq!(std::fs::read_to_string(&path).unwrap(), "kept\n");
        assert!(!slot.status().active);
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn setting_a_second_log_closes_the_first() {
        let dir = std::env::temp_dir().join(format!("harbour-log-swap-{}", uuid::Uuid::new_v4()));
        let slot = LogSlot::default();

        slot.set(OutputLog::start(&dir.join("a.log"), LogFormat::Plain, false).unwrap());
        slot.write(b"into a\n");
        slot.set(OutputLog::start(&dir.join("b.log"), LogFormat::Plain, false).unwrap());
        slot.write(b"into b\n");
        slot.clear();

        assert_eq!(
            std::fs::read_to_string(dir.join("a.log")).unwrap(),
            "into a\n"
        );
        assert_eq!(
            std::fs::read_to_string(dir.join("b.log")).unwrap(),
            "into b\n"
        );
        std::fs::remove_dir_all(dir).ok();
    }
}
