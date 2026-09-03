//! Owns every live session and mediates all access to it.

use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;

use parking_lot::{Mutex, RwLock};
use tokio::sync::mpsc;

use crate::error::{AppError, AppResult};
use crate::session::local::{self, SpawnOptions};
use crate::session::logging::LogSlot;
use crate::session::reader::Backpressure;
use crate::session::shell;
use crate::session::{SessionClosed, SessionId, SessionInfo, SessionKind, Transport};

pub struct SessionHandle {
    pub id: SessionId,
    pub kind: SessionKind,
    title: RwLock<String>,
    transport: Box<dyn Transport>,
    /// Taken exactly once, by `session_subscribe`.
    output: Mutex<Option<mpsc::Receiver<Vec<u8>>>>,
    pub backpressure: Backpressure,
    /// The session's output log, if one has been started. Shared with the
    /// sink so starting and stopping a log needs no restart of the pump.
    pub log: Arc<LogSlot>,
}

impl SessionHandle {
    pub fn info(&self) -> SessionInfo {
        SessionInfo {
            session_id: self.id.clone(),
            kind: self.kind,
            title: self.title.read().clone(),
        }
    }

    pub fn set_title(&self, title: impl Into<String>) {
        *self.title.write() = title.into();
    }

    pub fn write(&self, data: &[u8]) -> AppResult<()> {
        self.transport.write(data)
    }

    pub fn resize(&self, cols: u16, rows: u16) -> AppResult<()> {
        self.transport.resize(cols, rows)
    }

    /// Hands the raw output stream to the first caller; later callers get
    /// `ALREADY_SUBSCRIBED` rather than silently splitting the stream.
    pub fn take_output(&self) -> AppResult<mpsc::Receiver<Vec<u8>>> {
        self.output
            .lock()
            .take()
            .ok_or_else(|| AppError::AlreadySubscribed(self.id.clone()))
    }

    fn kill(&self) {
        self.transport.kill();
    }
}

#[derive(Default)]
pub struct SessionManager {
    sessions: RwLock<HashMap<SessionId, Arc<SessionHandle>>>,
}

pub struct OpenLocal {
    pub shell_id: Option<String>,
    pub cols: u16,
    pub rows: u16,
    pub cwd: Option<PathBuf>,
    pub env: Vec<(String, String)>,
}

/// A live connection waiting to be registered with the manager.
///
/// Anything that can produce a [`Transport`] and a byte stream - a pty today,
/// an SSH channel as of milestone 2 - becomes a session by handing one of
/// these to [`SessionManager::adopt`].
pub struct NewSession {
    pub id: SessionId,
    pub kind: SessionKind,
    pub title: String,
    pub transport: Box<dyn Transport>,
    pub output: mpsc::Receiver<Vec<u8>>,
}

/// Session ids are minted before the session exists, because the exit callback
/// has to be able to name the session it is reporting on.
pub fn new_id() -> SessionId {
    uuid::Uuid::new_v4().to_string()
}

impl SessionManager {
    pub fn new() -> Self {
        Self::default()
    }

    /// Opens a local shell session. `on_exit` runs once the child is reaped;
    /// the session has already been removed from the map by then.
    pub fn open_local<F>(&self, req: OpenLocal, on_exit: F) -> AppResult<SessionInfo>
    where
        F: FnOnce(SessionClosed) + Send + 'static,
    {
        let spec = match req.shell_id.as_deref() {
            Some(id) => shell::find(id).ok_or_else(|| AppError::ShellNotFound(id.to_string()))?,
            None => shell::detect()
                .into_iter()
                .next()
                .ok_or_else(|| AppError::ShellNotFound("default".into()))?,
        };

        let id = new_id();
        let exit_id = id.clone();

        let spawned = local::spawn(
            SpawnOptions {
                shell: &spec,
                cols: req.cols,
                rows: req.rows,
                cwd: req.cwd,
                env: req.env,
            },
            move |reason, code| {
                on_exit(SessionClosed::new(exit_id, reason, code));
            },
        )?;

        Ok(self.adopt(NewSession {
            id,
            kind: SessionKind::Local,
            title: spec.label.clone(),
            transport: Box::new(spawned.transport),
            output: spawned.output,
        }))
    }

    /// Takes ownership of a connected session and starts tracking it.
    pub fn adopt(&self, session: NewSession) -> SessionInfo {
        let handle = Arc::new(SessionHandle {
            id: session.id.clone(),
            kind: session.kind,
            title: RwLock::new(session.title),
            transport: session.transport,
            output: Mutex::new(Some(session.output)),
            backpressure: Backpressure::new(),
            log: Arc::new(LogSlot::default()),
        });

        let info = handle.info();
        self.sessions.write().insert(session.id, handle);
        info
    }

    pub fn get(&self, id: &str) -> AppResult<Arc<SessionHandle>> {
        self.sessions
            .read()
            .get(id)
            .cloned()
            .ok_or_else(|| AppError::SessionNotFound(id.to_string()))
    }

    /// Removes a session without killing it. Used by the exit path, where the
    /// child is already gone.
    pub fn remove(&self, id: &str) -> Option<Arc<SessionHandle>> {
        self.sessions.write().remove(id)
    }

    /// Removes a session and terminates its child.
    ///
    /// Returns as soon as the session is unreachable; the actual teardown runs
    /// on its own thread because releasing a pty can block (see [`teardown`]).
    pub fn close(&self, id: &str) -> AppResult<()> {
        let handle = self
            .sessions
            .write()
            .remove(id)
            .ok_or_else(|| AppError::SessionNotFound(id.to_string()))?;
        teardown(handle);
        Ok(())
    }

    pub fn list(&self) -> Vec<SessionInfo> {
        self.sessions.read().values().map(|h| h.info()).collect()
    }

    /// Kills every session; called on app exit so no orphan shells survive.
    pub fn close_all(&self) {
        let handles: Vec<_> = self.sessions.write().drain().map(|(_, h)| h).collect();
        for handle in handles {
            teardown(handle);
        }
    }
}

/// Kills a session's child and releases its pty off the caller's thread.
///
/// Dropping a pty master is not cheap and, on Windows, not even bounded:
/// `ClosePseudoConsole` blocks until the console's output buffer has been
/// drained. Doing that inline would stall the command handler - or, at app
/// exit, the UI thread - for as long as the console takes to wind down.
fn teardown(handle: Arc<SessionHandle>) {
    handle.kill();
    let spawned = std::thread::Builder::new()
        .name("harbour-pty-close".into())
        .spawn(move || drop(handle));
    if let Err(err) = spawned {
        // Out of threads: the drop happens here instead, which is slow but
        // still correct.
        tracing::warn!(error = %err, "could not spawn teardown thread");
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc as std_mpsc;
    use std::time::Duration;

    fn open_default(manager: &SessionManager) -> (SessionInfo, std_mpsc::Receiver<SessionClosed>) {
        let (tx, rx) = std_mpsc::channel();
        let info = manager
            .open_local(
                OpenLocal {
                    shell_id: None,
                    cols: 80,
                    rows: 24,
                    cwd: None,
                    env: vec![],
                },
                move |closed| {
                    let _ = tx.send(closed);
                },
            )
            .expect("the default shell should spawn");
        (info, rx)
    }

    #[test]
    fn opens_lists_and_closes_a_local_session() {
        let manager = SessionManager::new();
        let (info, _rx) = open_default(&manager);

        assert_eq!(manager.list().len(), 1);
        assert!(manager.get(&info.session_id).is_ok());

        manager.close(&info.session_id).unwrap();
        assert!(manager.list().is_empty());
        assert!(manager.get(&info.session_id).is_err());
    }

    #[test]
    fn unknown_session_reports_a_stable_code() {
        let manager = SessionManager::new();
        let err = manager
            .get("nope")
            .err()
            .expect("missing session is an error");
        assert_eq!(err.code(), "SESSION_NOT_FOUND");
    }

    #[test]
    fn unknown_shell_reports_a_stable_code() {
        let manager = SessionManager::new();
        let Err(err) = manager.open_local(
            OpenLocal {
                shell_id: Some("not-a-shell".into()),
                cols: 80,
                rows: 24,
                cwd: None,
                env: vec![],
            },
            |_| {},
        ) else {
            panic!("an unknown shell must be an error");
        };
        assert_eq!(err.code(), "SHELL_NOT_FOUND");
    }

    #[test]
    fn output_can_only_be_subscribed_once() {
        let manager = SessionManager::new();
        let (info, _rx) = open_default(&manager);
        let handle = manager.get(&info.session_id).unwrap();

        assert!(handle.take_output().is_ok());
        let Err(err) = handle.take_output() else {
            panic!("a second subscribe must be an error");
        };
        assert_eq!(err.code(), "ALREADY_SUBSCRIBED");
        manager.close(&info.session_id).unwrap();
    }

    #[test]
    fn writing_to_the_shell_produces_output() {
        let manager = SessionManager::new();
        let (info, _rx) = open_default(&manager);
        let handle = manager.get(&info.session_id).unwrap();
        let mut output = handle.take_output().unwrap();

        handle.write(b"\r").unwrap();
        handle.resize(100, 40).unwrap();

        // A shell always paints at least a prompt; 10s is generous for CI.
        let deadline = std::time::Instant::now() + Duration::from_secs(10);
        let mut got = Vec::new();
        while std::time::Instant::now() < deadline {
            match output.try_recv() {
                Ok(chunk) => {
                    got.extend_from_slice(&chunk);
                    break;
                }
                Err(_) => std::thread::sleep(Duration::from_millis(25)),
            }
        }

        manager.close(&info.session_id).unwrap();
        assert!(!got.is_empty(), "expected the shell to emit a prompt");
    }

    #[test]
    fn close_all_drains_every_session() {
        let manager = SessionManager::new();
        let (_a, _rx_a) = open_default(&manager);
        let (_b, _rx_b) = open_default(&manager);
        assert_eq!(manager.list().len(), 2);

        manager.close_all();
        assert!(manager.list().is_empty());
    }
}
