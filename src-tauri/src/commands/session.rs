//! Session IPC surface. See `docs/ipc.md` for the contract.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::ipc::{Channel, InvokeResponseBody};
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::session::logging::{LogSlot, LogStatus, OutputLog};
use crate::session::manager::OpenLocal;
use crate::session::reader::{self, Backpressure, OutputSink, SinkClosed};
use crate::session::SessionInfo;
use crate::settings::LogFormat;
use crate::AppState;

/// Bridges batched pty output onto a Tauri channel as raw bytes. Terminal
/// output must not be JSON-encoded: it is neither valid UTF-8 in general nor
/// cheap enough to stringify at 50 MB/s.
struct ChannelSink {
    channel: Channel<InvokeResponseBody>,
    /// The session log, if one is running. It sits here rather than inside the
    /// pump so that starting or stopping a log needs no restart of the pump -
    /// and so that what is logged is exactly what the terminal was sent.
    log: Arc<LogSlot>,
}

impl OutputSink for ChannelSink {
    fn send(&self, data: Vec<u8>) -> Result<(), SinkClosed> {
        self.log.write(&data);
        self.channel
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
    let sink = ChannelSink {
        channel: on_data,
        log: Arc::clone(&handle.log),
    };

    tauri::async_runtime::spawn(async move {
        reader::pump(output, sink, backpressure).await;
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

// ---------------------------------------------------------------------------
// Session logging
// ---------------------------------------------------------------------------

/// Starts writing this session's output to `path`.
///
/// Starting a log on a session that already has one replaces it, so "log
/// somewhere else" is one action rather than a stop and a start with a gap in
/// the middle. What was already on screen is not in the file: a log begins
/// when it is asked for.
#[tauri::command]
pub async fn session_log_start(
    state: State<'_, AppState>,
    session_id: String,
    path: String,
    format: LogFormat,
    append: bool,
) -> AppResult<LogStatus> {
    let handle = state.sessions.get(&session_id)?;
    let target = PathBuf::from(path);
    let log =
        tauri::async_runtime::spawn_blocking(move || OutputLog::start(&target, format, append))
            .await
            .map_err(|err| AppError::LogFailed(format!("log task failed: {err}")))??;

    let status = handle.log.set(log);
    tracing::info!(session = %session_id, path = ?status.path, "session logging started");
    Ok(status)
}

/// Stops logging and closes the file. Stopping a session that is not being
/// logged is success, not an error.
#[tauri::command]
pub async fn session_log_stop(
    state: State<'_, AppState>,
    session_id: String,
) -> AppResult<LogStatus> {
    let handle = state.sessions.get(&session_id)?;
    // Closing waits for the queued output to reach the disk, which is disk
    // work and does not belong on the runtime pumping terminal output.
    let log = Arc::clone(&handle.log);
    tauri::async_runtime::spawn_blocking(move || log.clear())
        .await
        .map_err(|err| AppError::LogFailed(format!("log task failed: {err}")))
}

#[tauri::command]
pub async fn session_log_status(
    state: State<'_, AppState>,
    session_id: String,
) -> AppResult<LogStatus> {
    Ok(state.sessions.get(&session_id)?.log.status())
}
