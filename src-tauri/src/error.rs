use serde::ser::SerializeStruct;
use serde::{Serialize, Serializer};

/// Every IPC failure carries a stable `code` so the frontend can branch on it
/// without string-matching human-readable messages.
#[derive(Debug, thiserror::Error)]
pub enum AppError {
    #[error("session {0} does not exist")]
    SessionNotFound(String),

    #[error("session {0} already has a subscriber")]
    AlreadySubscribed(String),

    #[error("shell `{0}` is not available on this machine")]
    ShellNotFound(String),

    #[error("failed to open a pseudo-terminal: {0}")]
    PtyOpen(String),

    #[error("failed to spawn `{program}`: {reason}")]
    Spawn { program: String, reason: String },

    #[error("io error: {0}")]
    Io(#[from] std::io::Error),

    #[error("{0}")]
    Internal(String),
}

impl AppError {
    /// Stable, machine-readable discriminant. Keep these in sync with
    /// `docs/ipc.md` and `src/ipc/types.ts`.
    pub fn code(&self) -> &'static str {
        match self {
            AppError::SessionNotFound(_) => "SESSION_NOT_FOUND",
            AppError::AlreadySubscribed(_) => "ALREADY_SUBSCRIBED",
            AppError::ShellNotFound(_) => "SHELL_NOT_FOUND",
            AppError::PtyOpen(_) => "PTY_OPEN_FAILED",
            AppError::Spawn { .. } => "SPAWN_FAILED",
            AppError::Io(_) => "IO_ERROR",
            AppError::Internal(_) => "INTERNAL",
        }
    }

    pub fn internal(msg: impl Into<String>) -> Self {
        AppError::Internal(msg.into())
    }
}

impl Serialize for AppError {
    fn serialize<S: Serializer>(&self, serializer: S) -> Result<S::Ok, S::Error> {
        let mut s = serializer.serialize_struct("AppError", 2)?;
        s.serialize_field("code", self.code())?;
        s.serialize_field("message", &self.to_string())?;
        s.end()
    }
}

pub type AppResult<T> = Result<T, AppError>;
