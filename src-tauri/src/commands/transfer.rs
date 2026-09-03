//! Transfer queue and open-in-editor IPC surface. See `docs/ipc.md`.
//!
//! Progress does not come back from these calls; it arrives as
//! `transfer:update` events carrying the whole transfer each time, and edits
//! the same way as `edit:update`. The commands here only start, steer and
//! stop things.

use std::sync::Arc;

use tauri::{AppHandle, State};
use tauri_plugin_opener::OpenerExt;

use crate::edit::EditInfo;
use crate::error::AppResult;
use crate::transfer::{ConflictPolicy, Request, Resolution, Transfer};
use crate::AppState;

/// Queues one transfer per item against `session_id`, whose SFTP channel is
/// opened if it is not already. Returns the transfers as queued; everything
/// after that is events.
#[tauri::command]
pub async fn transfer_enqueue(
    state: State<'_, AppState>,
    session_id: String,
    items: Vec<Request>,
    policy: ConflictPolicy,
) -> AppResult<Vec<Transfer>> {
    let sftp = state.connections.sftp(&session_id).await?;
    Ok(items
        .into_iter()
        .map(|request| {
            state
                .transfers
                .enqueue(session_id.clone(), Arc::clone(&sftp), request, policy)
        })
        .collect())
}

#[tauri::command]
pub async fn transfer_list(state: State<'_, AppState>) -> AppResult<Vec<Transfer>> {
    Ok(state.transfers.list())
}

#[tauri::command]
pub async fn transfer_pause(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.transfers.pause(&id)
}

#[tauri::command]
pub async fn transfer_resume(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.transfers.resume(&id)
}

#[tauri::command]
pub async fn transfer_cancel(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.transfers.cancel(&id)
}

/// Answers the conflict a transfer is stopped on. With `apply_to_all` the
/// same answer covers the rest of that transfer without asking again.
#[tauri::command]
pub async fn transfer_resolve(
    state: State<'_, AppState>,
    id: String,
    resolution: Resolution,
    apply_to_all: bool,
) -> AppResult<()> {
    state.transfers.resolve(&id, resolution, apply_to_all)
}

/// Forgets a finished transfer.
#[tauri::command]
pub async fn transfer_remove(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.transfers.remove(&id)
}

#[tauri::command]
pub async fn transfer_clear_finished(state: State<'_, AppState>) -> AppResult<usize> {
    Ok(state.transfers.clear_finished())
}

// ---------------------------------------------------------------------------
// Open in editor
// ---------------------------------------------------------------------------

/// Downloads a remote file to a private temporary directory, opens it with
/// the OS default for its type, and uploads it back on every save.
#[tauri::command]
pub async fn edit_open(
    app: AppHandle,
    state: State<'_, AppState>,
    session_id: String,
    path: String,
) -> AppResult<EditInfo> {
    let sftp = state.connections.sftp(&session_id).await?;
    let opener = app.opener();
    state
        .edits
        .open(session_id, sftp, &path, |local| {
            opener
                .open_path(local.display().to_string(), None::<&str>)
                .map_err(|err| err.to_string())
        })
        .await
}

#[tauri::command]
pub async fn edit_list(state: State<'_, AppState>) -> AppResult<Vec<EditInfo>> {
    Ok(state.edits.list())
}

/// Stops watching and removes the working copy. Anything not yet saved in the
/// editor is the editor's problem to warn about, as it would be for any file.
#[tauri::command]
pub async fn edit_close(state: State<'_, AppState>, id: String) -> AppResult<()> {
    state.edits.close(&id)
}
