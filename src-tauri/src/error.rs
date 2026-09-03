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

    #[error("could not reach {host}:{port}: {reason}")]
    SshConnect {
        host: String,
        port: u16,
        reason: String,
    },

    #[error("authentication failed for {user}@{host}: {reason}")]
    SshAuth {
        host: String,
        user: String,
        reason: String,
    },

    #[error("the host key for {host} was not accepted: {reason}")]
    SshHostKeyRejected { host: String, reason: String },

    #[error("the host key for {host} has changed; refusing to connect: {detail}")]
    SshHostKeyChanged { host: String, detail: String },

    #[error("ssh protocol error: {0}")]
    Ssh(#[from] russh::Error),

    #[error("could not load the key at {path}: {reason}")]
    SshKeyLoad { path: String, reason: String },

    #[error("no usable ssh agent: {0}")]
    SshAgent(String),

    #[error("the remote refused a channel: {0}")]
    SshChannel(String),

    #[error("no host with id {0}")]
    HostNotFound(String),

    #[error("no folder with id {0}")]
    FolderNotFound(String),

    #[error("the vault could not complete that: {0}")]
    Vault(String),

    #[error("the system keychain is not usable: {0}")]
    Keyring(String),

    #[error("the settings could not be saved: {0}")]
    Settings(String),

    #[error("could not import a colour scheme from {path}: {reason}")]
    SchemeImport { path: String, reason: String },

    #[error("could not import highlight rules from {path}: {reason}")]
    HighlightImport { path: String, reason: String },

    #[error("session logging failed: {0}")]
    LogFailed(String),

    #[error("sftp: {0}")]
    Sftp(String),

    #[error("{path}: {reason}")]
    Files { path: String, reason: String },

    #[error("prompt {0} is no longer waiting for an answer")]
    PromptNotFound(String),

    #[error("no answer to the prompt")]
    PromptTimedOut,

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
            AppError::SshConnect { .. } => "SSH_CONNECT_FAILED",
            AppError::SshAuth { .. } => "SSH_AUTH_FAILED",
            AppError::SshHostKeyRejected { .. } => "SSH_HOSTKEY_REJECTED",
            AppError::SshHostKeyChanged { .. } => "SSH_HOSTKEY_CHANGED",
            AppError::Ssh(_) => "SSH_PROTOCOL_ERROR",
            AppError::SshKeyLoad { .. } => "SSH_KEY_LOAD_FAILED",
            AppError::SshAgent(_) => "SSH_AGENT_UNAVAILABLE",
            AppError::SshChannel(_) => "SSH_CHANNEL_FAILED",
            AppError::HostNotFound(_) => "HOST_NOT_FOUND",
            AppError::FolderNotFound(_) => "FOLDER_NOT_FOUND",
            AppError::Vault(_) => "VAULT_ERROR",
            AppError::Keyring(_) => "KEYRING_UNAVAILABLE",
            AppError::Settings(_) => "SETTINGS_ERROR",
            AppError::SchemeImport { .. } => "SCHEME_IMPORT_FAILED",
            AppError::HighlightImport { .. } => "HIGHLIGHT_IMPORT_FAILED",
            AppError::LogFailed(_) => "LOG_FAILED",
            AppError::Sftp(_) => "SFTP_ERROR",
            AppError::Files { .. } => "FILES_ERROR",
            AppError::PromptNotFound(_) => "PROMPT_NOT_FOUND",
            AppError::PromptTimedOut => "PROMPT_TIMED_OUT",
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
