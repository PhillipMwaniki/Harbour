//! Transfer queue and open-in-editor IPC surface. See `docs/ipc.md`.
//!
//! Progress does not come back from these calls; it arrives as
//! `transfer:update` events carrying the whole transfer each time. The
//! commands here only start, steer and stop things.

use std::sync::Arc;

use tauri::State;

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
