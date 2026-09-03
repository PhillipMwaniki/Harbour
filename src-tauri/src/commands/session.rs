//! Session IPC surface. See `docs/ipc.md` for the contract.

use std::path::PathBuf;

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, State};

use crate::error::AppResult;
use crate::session::manager::OpenLocal;
use crate::session::reader::{self, Backpressure, OutputSink, SinkClosed};
use crate::session::SessionInfo;
use crate::AppState;

/// Bridges batched pty output onto a Tauri channel as raw bytes. Terminal
/// output must not be JSON-encoded: it is neither valid UTF-8 in general nor
/// cheap enough to stringify at 50 MB/s.
struct ChannelSink(Channel<InvokeResponseBody>);

impl OutputSink for ChannelSink {
    fn send(&self, data: Vec<u8>) -> Result<(), SinkClosed> {
        self.0
            .send(InvokeResponseBody::Raw(data))
            .map_err(|_| SinkClosed)
    }
}

#[tauri::command]
pub async fn session_open(
    app: AppHandle,
    state: State<'_, AppState>,
    shell_id: Option<String>,
    cols: u16,
    rows: u16,
    cwd: Option<String>,
) -> AppResult<SessionInfo> {
    let manager = state.sessions.clone();
    let exit_manager = state.sessions.clone();
    let exit_app = app.clone();

    let info = manager.open_local(
        OpenLocal {
            shell_id,
            cols,
            rows,
            cwd: cwd.map(PathBuf::from),
            env: Vec::new(),
        },
        move |closed| {
            exit_manager.remove(&closed.session_id);
            if let Err(err) = exit_app.emit("session:closed", &closed) {
                tracing::warn!(error = %err, "failed to emit session:closed");
            }
        },
    )?;

    tracing::info!(session = %info.session_id, "opened local session");
    let _ = app.emit("session:opened", &info);
    Ok(info)
}

/// Attaches the output stream. Must be called once per session, right after
/// `session_open`; output produced before this call is buffered, not lost.
#[tauri::command]
pub async fn session_subscribe(
    state: State<'_, AppState>,
    session_id: String,
    on_data: Channel<InvokeResponseBody>,
) -> AppResult<()> {
    let handle = state.sessions.get(&session_id)?;
    let output = handle.take_output()?;
    let backpressure: Backpressure = handle.backpressure.clone();

    tauri::async_runtime::spawn(async move {
        reader::pump(output, ChannelSink(on_data), backpressure).await;
        tracing::debug!(session = %session_id, "output pump finished");
    });

    Ok(())
}

#[tauri::command]
pub async fn session_write(
    state: State<'_, AppState>,
    session_id: String,
    data: Vec<u8>,
) -> AppResult<()> {
    state.sessions.get(&session_id)?.write(&data)
}

#[tauri::command]
pub async fn session_resize(
    state: State<'_, AppState>,
    session_id: String,
    cols: u16,
    rows: u16,
) -> AppResult<()> {
    state.sessions.get(&session_id)?.resize(cols, rows)
}

/// Acknowledges `bytes` of output as rendered, freeing that much of the
/// in-flight budget. Without this the pump stalls after 1 MB.
#[tauri::command]
pub async fn session_ack(
    state: State<'_, AppState>,
    session_id: String,
    bytes: usize,
) -> AppResult<()> {
    state.sessions.get(&session_id)?.backpressure.ack(bytes);
    Ok(())
}

#[tauri::command]
pub async fn session_set_title(
    state: State<'_, AppState>,
    session_id: String,
    title: String,
) -> AppResult<()> {
    state.sessions.get(&session_id)?.set_title(title);
    Ok(())
}

#[tauri::command]
pub async fn session_close(state: State<'_, AppState>, session_id: String) -> AppResult<()> {
    state.sessions.close(&session_id)
}

#[tauri::command]
pub async fn session_list(state: State<'_, AppState>) -> AppResult<Vec<SessionInfo>> {
    Ok(state.sessions.list())
}
