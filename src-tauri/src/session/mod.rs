pub mod local;
pub mod logging;
pub mod manager;
pub mod reader;
pub mod shell;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;

pub type SessionId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    /// A local shell attached to a pty.
    Local,
    /// A shell on an SSH channel.
    Ssh,
    /// A telnet connection over a raw TCP socket.
    Telnet,
    /// A serial console on a local port.
    Serial,
}

/// What a session is attached to, from the manager's point of view.
///
/// A pty and an SSH channel differ in almost every respect except the three
/// things the session layer needs from them, which are these. Output does not
/// appear here: it is a `mpsc::Receiver<Vec<u8>>` handed over at construction
/// and drained by [`reader::pump`], the same for both.
///
/// Every method must return promptly. `kill` in particular is called from the
/// teardown path, which must not block on a console winding down or a TCP
/// connection closing - see [`manager::teardown`].
pub trait Transport: Send + Sync + 'static {
    fn write(&self, data: &[u8]) -> AppResult<()>;
    fn resize(&self, cols: u16, rows: u16) -> AppResult<()>;
    fn kill(&self);
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: SessionId,
    pub kind: SessionKind,
    pub title: String,
}

/// Why a session ended. The distinction matters to the user: a shell they
/// exited, a tab they closed and a connection that died under them look the
/// same in a tab that simply disappears.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum ExitReason {
    /// The process or remote shell ended on its own.
    Exited,
    /// Harbour ended it: a closed tab, or the app shutting down.
    Killed,
    /// The pty or the connection went away underneath us.
    Lost,
}

impl ExitReason {
    /// The wire value. Stable; see `docs/ipc.md`.
    pub fn as_str(self) -> &'static str {
        match self {
            ExitReason::Exited => "exit",
            ExitReason::Killed => "killed",
            ExitReason::Lost => "error",
        }
    }
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionClosed {
    pub session_id: SessionId,
    /// `exit`, `killed`, or `error`.
    pub reason: String,
    pub exit_code: Option<u32>,
}

impl SessionClosed {
    pub fn new(session_id: SessionId, reason: ExitReason, exit_code: Option<u32>) -> Self {
        Self {
            session_id,
            reason: reason.as_str().to_string(),
            exit_code,
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// These strings are the wire contract in `docs/ipc.md`, and the frontend
    /// branches on them. Renaming a variant must not silently rename them.
    #[test]
    fn exit_reasons_keep_their_wire_names() {
        assert_eq!(ExitReason::Exited.as_str(), "exit");
        assert_eq!(ExitReason::Killed.as_str(), "killed");
        assert_eq!(ExitReason::Lost.as_str(), "error");
    }

    #[test]
    fn a_closed_session_serialises_as_the_frontend_expects() {
        let closed = SessionClosed::new("s1".into(), ExitReason::Exited, Some(130));
        let json = serde_json::to_value(&closed).unwrap();

        assert_eq!(json["sessionId"], "s1");
        assert_eq!(json["reason"], "exit");
        assert_eq!(json["exitCode"], 130);
    }

    #[test]
    fn a_session_that_never_reported_a_status_has_a_null_code() {
        let closed = SessionClosed::new("s1".into(), ExitReason::Lost, None);
        let json = serde_json::to_value(&closed).unwrap();

        assert_eq!(json["reason"], "error");
        assert!(json["exitCode"].is_null());
    }
}
