//! Serial IPC surface. See `docs/ipc.md` for the contract.

use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::serial::{self, PortInfo};
use crate::session::manager::{self, NewSession};
use crate::session::{SessionClosed, SessionInfo, SessionKind};
use crate::AppState;

/// Runs a blocking serial operation off the async runtime.
async fn blocking<T, F>(work: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| AppError::internal(format!("serial task failed: {err}")))?
}

/// Lists the serial ports currently attached, for the connect dialog.
#[tauri::command]
pub async fn serial_ports() -> AppResult<Vec<PortInfo>> {
    blocking(serial::ports).await
}

/// Opens a serial console on `path` at `baud`.
#[tauri::command]
pub async fn serial_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    path: String,
    baud: u32,
) -> AppResult<SessionInfo> {
    let path = path.trim().to_string();
    if path.is_empty() {
        return Err(AppError::internal("a port is required"));
    }
    if baud == 0 {
        return Err(AppError::internal("a baud rate is required"));
    }

    let id = manager::new_id();
    let exit_id = id.clone();
    let exit_manager = state.sessions.clone();
    let exit_app = app.clone();

    let label = format!("{path} @ {baud}");
    let open_path = path.clone();
    let connected = blocking(move || {
        serial::open(&open_path, baud, move |reason, code| {
            let closed = SessionClosed::new(exit_id, reason, code);
            exit_manager.remove(&closed.session_id);
            if let Err(err) = exit_app.emit("session:closed", &closed) {
                tracing::warn!(error = %err, "failed to emit session:closed");
            }
        })
    })
    .await?;

    tracing::info!(session = %id, port = %path, baud, "opened serial session");

    let info = state.sessions.adopt(NewSession {
        id,
        kind: SessionKind::Serial,
        title: label,
        transport: Box::new(connected.transport),
        output: connected.output,
    });

    let _ = app.emit("session:opened", &info);
    Ok(info)
}
