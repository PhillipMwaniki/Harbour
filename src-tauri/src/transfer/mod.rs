//! Moving files between this machine and a remote one.
//!
//! A transfer is one source path to one destination path - a file, or a
//! directory and everything under it - queued against the SSH session whose
//! SFTP channel it rides. The engine runs a bounded number per session,
//! reports progress as it goes, and stops at every file that already exists
//! at the destination to ask, unless told in advance what to do.
//!
//! Nothing here is persisted: the queue lives as long as the app does, and a
//! transfer a session takes with it when it closes is cancelled, not lost
//! silently - its last state says so.

pub mod copy;
pub mod engine;

use serde::{Deserialize, Serialize};

pub type TransferId = String;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Direction {
    /// Local -> remote.
    Upload,
    /// Remote -> local.
    Download,
}

/// What to do when a file already exists at the destination.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum ConflictPolicy {
    /// Stop and ask, once per file, with the option to apply the answer to
    /// the rest of the transfer.
    #[default]
    Ask,
    Overwrite,
    Skip,
    /// Continue a partial copy from where it stopped, by size. A destination
    /// that is already complete is left alone; one that is *larger* than the
    /// source is overwritten, since it cannot be a partial copy of it.
    Resume,
    /// Write beside it as `name (1).ext`.
    Rename,
}

/// The answer to one conflict.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Resolution {
    Overwrite,
    Skip,
    Resume,
    Rename,
    Cancel,
}

impl Resolution {
    /// The policy that makes this answer automatic for the rest of a transfer.
    pub fn as_policy(self) -> Option<ConflictPolicy> {
        match self {
            Resolution::Overwrite => Some(ConflictPolicy::Overwrite),
            Resolution::Skip => Some(ConflictPolicy::Skip),
            Resolution::Resume => Some(ConflictPolicy::Resume),
            Resolution::Rename => Some(ConflictPolicy::Rename),
            Resolution::Cancel => None,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum TransferState {
    /// Waiting for a slot on its session.
    Queued,
    Running,
    Paused,
    /// Stopped at a file that exists; `Transfer::conflict` says which.
    Conflict,
    Done,
    /// Every file was skipped by policy, so nothing was written.
    Skipped,
    Cancelled,
    Failed,
}

impl TransferState {
    pub fn is_finished(self) -> bool {
        matches!(
            self,
            TransferState::Done
                | TransferState::Skipped
                | TransferState::Cancelled
                | TransferState::Failed
        )
    }
}

/// Everything the conflict prompt needs to show for one file.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ConflictInfo {
    /// The destination that already exists.
    pub path: String,
    pub source_size: u64,
    pub source_modified: Option<i64>,
    pub destination_size: u64,
    pub destination_modified: Option<i64>,
    /// Whether the destination is smaller than the source, so that resuming
    /// is a meaningful answer.
    pub resumable: bool,
}

/// One thing to copy. `destination` is the full target path - for a
/// directory, the directory that will be created - so a rename on conflict
/// has one well-defined thing to rename.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Request {
    pub direction: Direction,
    pub source: String,
    pub destination: String,
}

/// A transfer as the frontend sees it: the whole state in one object, sent on
/// every change, so the UI never has to reconstruct anything from a stream of
/// deltas it might have missed.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Transfer {
    pub id: TransferId,
    pub session_id: String,
    pub direction: Direction,
    pub source: String,
    pub destination: String,
    pub state: TransferState,
    pub conflict: Option<ConflictInfo>,
    pub bytes_done: u64,
    /// Known once the transfer has been planned; zero before that.
    pub bytes_total: u64,
    pub files_done: u32,
    pub files_total: u32,
    /// The file being copied right now, for a directory transfer.
    pub current_file: Option<String>,
    pub error: Option<String>,
    /// Seconds since the epoch.
    pub queued_at: i64,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finished_states_are_the_ones_that_free_a_slot() {
        assert!(TransferState::Done.is_finished());
        assert!(TransferState::Skipped.is_finished());
        assert!(TransferState::Cancelled.is_finished());
        assert!(TransferState::Failed.is_finished());
        assert!(!TransferState::Queued.is_finished());
        assert!(!TransferState::Running.is_finished());
        assert!(!TransferState::Paused.is_finished());
        assert!(!TransferState::Conflict.is_finished());
    }

    #[test]
    fn every_resolution_but_cancel_becomes_a_policy() {
        assert_eq!(
            Resolution::Overwrite.as_policy(),
            Some(ConflictPolicy::Overwrite)
        );
        assert_eq!(Resolution::Skip.as_policy(), Some(ConflictPolicy::Skip));
        assert_eq!(Resolution::Resume.as_policy(), Some(ConflictPolicy::Resume));
        assert_eq!(Resolution::Rename.as_policy(), Some(ConflictPolicy::Rename));
        assert_eq!(Resolution::Cancel.as_policy(), None);
    }

    #[test]
    fn wire_names_are_lowercase_and_camel_case() {
        let request: Request =
            serde_json::from_str(r#"{"direction":"upload","source":"a","destination":"b"}"#)
                .unwrap();
        assert_eq!(request.direction, Direction::Upload);
        let json = serde_json::to_value(TransferState::Conflict).unwrap();
        assert_eq!(json, "conflict");
        let policy: ConflictPolicy = serde_json::from_str(r#""rename""#).unwrap();
        assert_eq!(policy, ConflictPolicy::Rename);
    }
}
