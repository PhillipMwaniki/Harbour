//! Port forwarding IPC surface. See `docs/ipc.md`.
//!
//! Like transfers, forwards report through `forward:update` events carrying
//! the whole forward; these commands only open, list and close them.

use std::sync::Arc;

use tauri::State;

use crate::error::AppResult;
use crate::ssh::forward::{ForwardInfo, ForwardSpec};
use crate::AppState;

/// Opens a local port forward on `session_id`'s connection. The bind happens
/// now, so a port already in use is an error here rather than later.
#[tauri::command]
pub async fn forward_open_local(
    state: State<'_, AppState>,
    session_id: String,
    spec: ForwardSpec,
) -> AppResult<ForwardInfo> {
    // A forward rides an existing SSH connection; opening the SFTP-style
    // channel opener is how it reaches it. A local shell has none.
    let opener = state.connections.opener(&session_id)?;
    state.forwards.open_local(session_id, opener, spec).await
}

/// Opens a dynamic (SOCKS5) forward on `session_id`'s connection - `ssh -D`.
/// Applications point their SOCKS proxy at the bound port.
#[tauri::command]
pub async fn forward_open_dynamic(
    state: State<'_, AppState>,
    session_id: String,
    bind_address: String,
    local_port: u16,
) -> AppResult<ForwardInfo> {
    let opener = state.connections.opener(&session_id)?;
    state
        .forwards
        .open_dynamic(session_id, opener, bind_address, local_port)
        .await
}

#[tauri::command]
pub async fn forward_list(state: State<'_, AppState>) -> AppResult<Vec<ForwardInfo>> {
    Ok(state.forwards.list())
}

#[tauri::command]
pub async fn forward_close(state: State<'_, AppState>, id: String) -> AppResult<()> {
    let _ = Arc::clone(&state.forwards);
    state.forwards.close(&id)
}
