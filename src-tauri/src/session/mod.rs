pub mod local;
pub mod manager;
pub mod reader;
pub mod shell;

use serde::{Deserialize, Serialize};

pub type SessionId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum SessionKind {
    /// A local shell attached to a pty. SSH and serial join this enum later.
    Local,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionInfo {
    pub session_id: SessionId,
    pub kind: SessionKind,
    pub title: String,
}

#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SessionClosed {
    pub session_id: SessionId,
    /// `exit`, `killed`, or `error`.
    pub reason: String,
    pub exit_code: Option<u32>,
}
