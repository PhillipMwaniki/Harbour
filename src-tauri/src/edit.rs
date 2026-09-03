//! Open a remote file in the local editor, and put it back when it is saved.
//!
//! The file is downloaded to a private temporary directory, handed to
//! whatever the OS opens that kind of file with, and watched. Every save
//! uploads it again, in place, so `vim` over SFTP feels like `vim` on the
//! machine. Closing the edit - or the session - stops the watcher and removes
//! the copy.
//!
//! Editors save in different ways: some write in place, some write a new file
//! and rename it over the old one. Watching the *directory* and matching on
//! the file name catches both, and a short debounce turns the two or three
//! events a single save produces into one upload.

use std::collections::HashMap;
use std::path::{Path, PathBuf};
use std::sync::Arc;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use notify::{RecommendedWatcher, RecursiveMode, Watcher};
use parking_lot::Mutex;
use russh_sftp::client::SftpSession;
use russh_sftp::protocol::OpenFlags;
use serde::Serialize;
use tokio::io::AsyncWriteExt;
use tokio::sync::mpsc;

use crate::error::{AppError, AppResult};
use crate::session::SessionId;

pub type EditId = String;

/// A save has to settle before it is uploaded: the editor may still be
/// writing, and a second event for the same save is usual.
const SETTLE: Duration = Duration::from_millis(400);

#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct EditInfo {
    pub id: EditId,
    pub session_id: SessionId,
    pub remote_path: String,
    pub local_path: String,
    /// How many saves have been uploaded so far.
    pub uploads: u32,
    /// Seconds since the epoch, for the last upload.
    pub last_upload: Option<i64>,
    /// The last upload failed; the local copy still has the user's work.
    pub error: Option<String>,
    pub closed: bool,
}

pub type Emitter = Arc<dyn Fn(&EditInfo) + Send + Sync>;

struct Edit {
    info: EditInfo,
    directory: PathBuf,
    /// Dropping the watcher is what stops it.
    _watcher: RecommendedWatcher,
    task: tauri::async_runtime::JoinHandle<()>,
}

pub struct Editor {
    edits: Mutex<HashMap<EditId, Edit>>,
    emit: Emitter,
}

fn now() -> i64 {
    SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .map(|since| since.as_secs() as i64)
        .unwrap_or(0)
}

impl Editor {
    pub fn new(emit: Emitter) -> Arc<Self> {
        Arc::new(Self {
            edits: Mutex::new(HashMap::new()),
            emit,
        })
    }

    /// Where the working copies live. Each edit gets its own directory under
    /// it, so two files with the same name from two hosts never collide.
    pub fn root() -> PathBuf {
        std::env::temp_dir().join("harbour").join("edits")
    }

    /// Downloads `remote_path`, opens it with `launch`, and starts watching.
    ///
    /// `launch` is passed in rather than done here so this module never
    /// depends on the app handle - and so a test can open a file without
    /// spawning an editor.
    pub async fn open(
        self: &Arc<Self>,
        session_id: SessionId,
        sftp: Arc<SftpSession>,
        remote_path: &str,
        launch: impl FnOnce(&Path) -> Result<(), String>,
    ) -> AppResult<EditInfo> {
        let name = remote_path
            .rsplit('/')
            .next()
            .filter(|name| !name.is_empty())
            .unwrap_or("file");
        let id = uuid::Uuid::new_v4().to_string();
        let directory = Self::root().join(&id);
        let local_path = directory.join(name);

        let bytes = sftp
            .read(remote_path)
            .await
            .map_err(|err| AppError::Edit(format!("could not read {remote_path}: {err}")))?;
        tokio::fs::create_dir_all(&directory).await.map_err(|err| {
            AppError::Edit(format!("could not create {}: {err}", directory.display()))
        })?;
        tokio::fs::write(&local_path, &bytes).await.map_err(|err| {
            AppError::Edit(format!("could not write {}: {err}", local_path.display()))
        })?;

        if let Err(reason) = launch(&local_path) {
            let _ = std::fs::remove_dir_all(&directory);
            return Err(AppError::Edit(format!(
                "could not open an editor: {reason}"
            )));
        }

        let info = EditInfo {
            id: id.clone(),
            session_id,
            remote_path: remote_path.to_string(),
            local_path: local_path.display().to_string(),
            uploads: 0,
            last_upload: None,
            error: None,
            closed: false,
        };

        let (events_tx, events_rx) = mpsc::unbounded_channel::<notify::Result<notify::Event>>();
        let mut watcher = notify::recommended_watcher(move |event| {
            // The callback runs on the watcher's own thread; the send never
            // blocks, and a closed receiver just means the edit is over.
            let _ = events_tx.send(event);
        })
        .map_err(|err| AppError::Edit(format!("could not watch for saves: {err}")))?;
        watcher
            .watch(&directory, RecursiveMode::NonRecursive)
            .map_err(|err| {
                AppError::Edit(format!("could not watch {}: {err}", directory.display()))
            })?;

        let task = tauri::async_runtime::spawn(Arc::clone(self).watch(
            id.clone(),
            sftp,
            local_path.clone(),
            events_rx,
        ));

        self.edits.lock().insert(
            id,
            Edit {
                info: info.clone(),
                directory,
                _watcher: watcher,
                task,
            },
        );
        (self.emit)(&info);
        Ok(info)
    }

    /// Turns file system events into uploads, one per settled save.
    async fn watch(
        self: Arc<Self>,
        id: EditId,
        sftp: Arc<SftpSession>,
        local_path: PathBuf,
        mut events: mpsc::UnboundedReceiver<notify::Result<notify::Event>>,
    ) {
        let name = local_path.file_name().map(|n| n.to_os_string());
        let concerns_us = |event: &notify::Event| {
            event
                .paths
                .iter()
                .any(|path| path.file_name().map(|n| n.to_os_string()) == name)
        };

        while let Some(event) = events.recv().await {
            let Ok(event) = event else { continue };
            if !concerns_us(&event) {
                continue;
            }
            // Let the save finish, and swallow the events it still produces.
            loop {
                match tokio::time::timeout(SETTLE, events.recv()).await {
                    Ok(Some(_)) => continue,
                    Ok(None) => return,
                    Err(_) => break,
                }
            }
            self.upload(&id, &sftp, &local_path).await;
        }
    }

    async fn upload(&self, id: &str, sftp: &SftpSession, local_path: &Path) {
        let remote_path = match self.edits.lock().get(id) {
            Some(edit) => edit.info.remote_path.clone(),
            None => return,
        };

        // An editor that writes-then-renames can leave a window where the
        // file is briefly absent; one retry covers it.
        let mut bytes = tokio::fs::read(local_path).await;
        if bytes.is_err() {
            tokio::time::sleep(SETTLE).await;
            bytes = tokio::fs::read(local_path).await;
        }

        let result = match bytes {
            Ok(bytes) => write_whole(sftp, &remote_path, &bytes).await,
            Err(err) => Err(err.to_string()),
        };

        let info = {
            let mut edits = self.edits.lock();
            let Some(edit) = edits.get_mut(id) else {
                return;
            };
            match result {
                Ok(()) => {
                    edit.info.uploads += 1;
                    edit.info.last_upload = Some(now());
                    edit.info.error = None;
                }
                Err(reason) => edit.info.error = Some(reason),
            }
            edit.info.clone()
        };
        (self.emit)(&info);
    }

    pub fn list(&self) -> Vec<EditInfo> {
        self.edits
            .lock()
            .values()
            .map(|edit| edit.info.clone())
            .collect()
    }

    /// Stops watching and removes the working copy.
    pub fn close(&self, id: &str) -> AppResult<()> {
        let edit = self
            .edits
            .lock()
            .remove(id)
            .ok_or_else(|| AppError::Edit(format!("no edit {id}")))?;
        edit.task.abort();
        let _ = std::fs::remove_dir_all(&edit.directory);
        let mut info = edit.info;
        info.closed = true;
        (self.emit)(&info);
        Ok(())
    }

    /// The session is gone; so is every edit that was uploading to it.
    pub fn close_session(&self, session_id: &str) {
        let ids: Vec<EditId> = self
            .edits
            .lock()
            .values()
            .filter(|edit| edit.info.session_id == session_id)
            .map(|edit| edit.info.id.clone())
            .collect();
        for id in ids {
            let _ = self.close(&id);
        }
    }
}

/// Replaces a remote file's contents. `SftpSession::write` does not truncate,
/// so a save shorter than the file it replaces would leave the old tail behind
/// - which is exactly the kind of corruption an editor must never cause.
async fn write_whole(sftp: &SftpSession, path: &str, bytes: &[u8]) -> Result<(), String> {
    let mut file = sftp
        .open_with_flags(
            path,
            OpenFlags::CREATE | OpenFlags::TRUNCATE | OpenFlags::WRITE,
        )
        .await
        .map_err(|err| err.to_string())?;
    file.write_all(bytes).await.map_err(|err| err.to_string())?;
    file.flush().await.map_err(|err| err.to_string())?;
    file.close().await.map_err(|err| err.to_string())
}
