//! File pane IPC surface: the remote side over SFTP, the local side over
//! `std::fs`. See `docs/ipc.md` for the contract.
//!
//! Nothing here writes to either file system. Milestone 5 is looking;
//! milestone 6 brings the transfers, and with them the first mutation.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::files::{local, Listing};
use crate::ssh::sftp;
use crate::AppState;

async fn blocking<T, F>(work: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| AppError::internal(format!("file task failed: {err}")))?
}

// ---------------------------------------------------------------------------
// Remote
// ---------------------------------------------------------------------------

/// The remote login directory. Opens the session's SFTP channel if it is not
/// open yet, which is also how the pane learns whether the server has SFTP
/// at all.
#[tauri::command]
pub async fn sftp_home(state: State<'_, AppState>, session_id: String) -> AppResult<String> {
    let connections = Arc::clone(&state.connections);
    let session = connections.sftp(&session_id).await?;
    sftp::home(&session).await
}

#[tauri::command]
pub async fn sftp_list(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
) -> AppResult<Listing> {
    let connections = Arc::clone(&state.connections);
    let session = connections.sftp(&session_id).await?;
    sftp::list(&session, &path).await
}

/// Closes the session's SFTP channel; the terminal stays. A later `sftp_*`
/// call opens a fresh one.
#[tauri::command]
pub async fn sftp_close(state: State<'_, AppState>, session_id: String) -> AppResult<()> {
    let connections = Arc::clone(&state.connections);
    connections.close_sftp(&session_id).await;
    Ok(())
}

// ---------------------------------------------------------------------------
// Local
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn local_home() -> AppResult<String> {
    local::home()
}

/// Every drive on Windows, `/` elsewhere: what "up" from a root offers.
#[tauri::command]
pub async fn local_roots() -> AppResult<Vec<String>> {
    // Probing drive letters touches the disk, and a card reader with no card
    // in it can take a moment to say so.
    blocking(|| Ok(local::roots())).await
}

#[tauri::command]
pub async fn local_list(path: String) -> AppResult<Listing> {
    let path = PathBuf::from(path);
    blocking(move || local::list(&path)).await
}
