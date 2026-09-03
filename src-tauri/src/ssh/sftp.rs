//! SFTP on the connection a terminal is already using.
//!
//! An SSH connection carries any number of channels, and SFTP is just one more:
//! a channel with the `sftp` subsystem on it. So the file pane never
//! connects or authenticates: it asks the terminal's connection for a second
//! channel, and rides the trust and credentials that connection already
//! established. One host key prompt, one password, however many panes.
//!
//! The connection `Handle` is owned by the transport's writer task and is not
//! `Clone`, so the channel is opened *through* that task - see
//! [`ChannelOpener`] - rather than by holding a second reference to the
//! connection. The connection stays owned in exactly one place, and closing
//! the terminal still closes everything.

use std::collections::HashMap;
use std::sync::Arc;

use parking_lot::Mutex;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::FileType;

use crate::error::{AppError, AppResult};
use crate::files::{posix_join, posix_parent, Entry, EntryKind, Listing};
use crate::session::SessionId;
use crate::ssh::client::wait_for_reply;
use crate::ssh::transport::ChannelOpener;

fn sftp_error(err: impl std::fmt::Display) -> AppError {
    AppError::Sftp(err.to_string())
}

fn path_error(path: &str, err: impl std::fmt::Display) -> AppError {
    AppError::Files {
        path: path.to_string(),
        reason: err.to_string(),
    }
}

/// Opens the `sftp` subsystem on a new channel of an existing connection.
pub async fn open(opener: &ChannelOpener) -> AppResult<SftpSession> {
    let mut channel = opener.open().await?;
    channel
        .request_subsystem(true, "sftp")
        .await
        .map_err(|err| AppError::SshChannel(err.to_string()))?;
    // A server with no SFTP subsystem says so here, which is the one message
    // worth giving the user verbatim.
    wait_for_reply(&mut channel, "sftp subsystem").await?;
    SftpSession::new(channel.into_stream())
        .await
        .map_err(sftp_error)
}

/// Where the remote side starts: the login directory, made absolute.
pub async fn home(sftp: &SftpSession) -> AppResult<String> {
    sftp.canonicalize(".").await.map_err(sftp_error)
}

/// Lists a remote directory. The path comes back canonical, so `..` and
/// symlinked directories resolve to where the user actually is.
pub async fn list(sftp: &SftpSession, path: &str) -> AppResult<Listing> {
    let requested = match path.trim() {
        "" => ".",
        other => other,
    };
    let canonical = sftp
        .canonicalize(requested)
        .await
        .map_err(|err| path_error(requested, err))?;
    let read = sftp
        .read_dir(&canonical)
        .await
        .map_err(|err| path_error(&canonical, err))?;

    let mut entries = Vec::new();
    for item in read {
        let name = item.file_name();
        let own = item.metadata();
        let symlink = matches!(own.file_type(), FileType::Symlink);

        // The listing says what an entry *is*; following a link says what it
        // points at, which is what decides whether a double-click enters it.
        // A dangling link is `Other`, and listed rather than hidden.
        let target = if symlink {
            sftp.metadata(posix_join(&canonical, &name)).await.ok()
        } else {
            Some(own.clone())
        };
        let kind = match target.as_ref().map(|meta| meta.file_type()) {
            Some(FileType::Dir) => EntryKind::Dir,
            Some(FileType::File) => EntryKind::File,
            _ => EntryKind::Other,
        };

        entries.push(Entry {
            hidden: name.starts_with('.'),
            name,
            kind,
            symlink,
            size: target
                .as_ref()
                .filter(|_| kind == EntryKind::File)
                .and_then(|meta| meta.size),
            modified: target.as_ref().and_then(|meta| meta.mtime).map(i64::from),
            permissions: target
                .as_ref()
                .and_then(|meta| meta.permissions)
                .map(|mode| mode & 0o7777),
            owner: own.user.clone(),
            group: own.group.clone(),
        });
    }

    Ok(Listing {
        parent: posix_parent(&canonical),
        path: canonical,
        entries,
    })
}

struct Connection {
    opener: ChannelOpener,
    /// Opened on first use and kept for the life of the session: a listing
    /// is a round trip on an existing channel, not a new channel each time.
    sftp: Option<Arc<SftpSession>>,
}

/// The SSH connections behind live sessions, by session id.
///
/// Kept beside the session manager rather than inside it: the manager knows
/// about transports and nothing about SSH, and this is the one place that
/// needs to. A local shell has no entry here, which is how `sftp_*` commands
/// learn that a session has no remote side.
#[derive(Default)]
pub struct Connections {
    inner: Mutex<HashMap<SessionId, Connection>>,
}

impl Connections {
    pub fn new() -> Arc<Self> {
        Arc::new(Self::default())
    }

    pub fn register(&self, session_id: SessionId, opener: ChannelOpener) {
        self.inner
            .lock()
            .insert(session_id, Connection { opener, sftp: None });
    }

    /// Forgets a session. The SFTP channel, if one was open, dies with the
    /// connection; nothing here has to wait for it.
    pub fn remove(&self, session_id: &str) {
        self.inner.lock().remove(session_id);
    }

    pub fn has(&self, session_id: &str) -> bool {
        self.inner.lock().contains_key(session_id)
    }

    /// The session's SFTP channel, opened on first use.
    pub async fn sftp(&self, session_id: &str) -> AppResult<Arc<SftpSession>> {
        let (opener, existing) = {
            let guard = self.inner.lock();
            let connection = guard.get(session_id).ok_or_else(|| {
                AppError::Sftp(format!(
                    "session {session_id} is not an SSH session, or has already closed"
                ))
            })?;
            (connection.opener.clone(), connection.sftp.clone())
        };
        if let Some(sftp) = existing {
            return Ok(sftp);
        }

        let fresh = Arc::new(open(&opener).await?);

        let mut guard = self.inner.lock();
        match guard.get_mut(session_id) {
            Some(connection) => {
                // Two panes asking at once must not each get their own channel.
                if let Some(theirs) = &connection.sftp {
                    return Ok(Arc::clone(theirs));
                }
                connection.sftp = Some(Arc::clone(&fresh));
                Ok(fresh)
            }
            None => Err(AppError::Sftp(
                "the session closed while its file channel was opening".into(),
            )),
        }
    }

    /// Closes the SFTP channel but keeps the session. The next `sftp` call
    /// opens a new one.
    pub async fn close_sftp(&self, session_id: &str) {
        let taken = self
            .inner
            .lock()
            .get_mut(session_id)
            .and_then(|connection| connection.sftp.take());
        if let Some(sftp) = taken {
            let _ = sftp.close().await;
        }
    }
}
