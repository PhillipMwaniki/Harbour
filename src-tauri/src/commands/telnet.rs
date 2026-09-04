//! Telnet IPC surface. See `docs/ipc.md` for the contract.
//!
//! Telnet has no authentication of its own and no host key: it is a raw TCP
//! connection, so opening one is a single command with no round trips. Whatever
//! login the far end wants happens in the terminal, like any other prompt.

use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::session::manager::{self, NewSession};
use crate::session::{SessionClosed, SessionInfo, SessionKind};
use crate::{telnet, AppState};

/// The port telnet uses when the caller does not name one.
const DEFAULT_TELNET_PORT: u16 = 23;

/// Opens a telnet session to `host:port`.
#[tauri::command]
pub async fn telnet_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    host: String,
    port: u16,
    cols: u16,
    rows: u16,
) -> AppResult<SessionInfo> {
    let host = host.trim().to_string();
    if host.is_empty() {
        return Err(AppError::internal("a host is required"));
    }
    let port = if port == 0 { DEFAULT_TELNET_PORT } else { port };

    let id = manager::new_id();
    let exit_id = id.clone();
    let exit_manager = state.sessions.clone();
    let exit_app = app.clone();

    let label = if port == DEFAULT_TELNET_PORT {
        host.clone()
    } else {
        format!("{host}:{port}")
    };

    let connected = telnet::connect(&host, port, cols, rows, move |reason, code| {
        let closed = SessionClosed::new(exit_id, reason, code);
        exit_manager.remove(&closed.session_id);
        if let Err(err) = exit_app.emit("session:closed", &closed) {
            tracing::warn!(error = %err, "failed to emit session:closed");
        }
    })
    .await?;

    tracing::info!(session = %id, host = %host, port, "opened telnet session");

    let info = state.sessions.adopt(NewSession {
        id,
        kind: SessionKind::Telnet,
        title: label,
        transport: Box::new(connected.transport),
        output: connected.output,
    });

    let _ = app.emit("session:opened", &info);
    Ok(info)
}
