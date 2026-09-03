pub mod commands;
pub mod error;
pub mod session;
pub mod telemetry;
pub mod vault;

use std::sync::Arc;

use tauri::Manager;

use crate::session::manager::SessionManager;

/// Everything the command handlers need. Kept deliberately small: each
/// subsystem (vault, sftp, forwards) adds its own field as it lands.
pub struct AppState {
    pub sessions: Arc<SessionManager>,
}

#[cfg_attr(mobile, tauri::mobile_entry_point)]
pub fn run() {
    tauri::Builder::default()
        .plugin(tauri_plugin_opener::init())
        .setup(|app| {
            let log_dir = app
                .path()
                .app_log_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("harbour"));
            // The guard must outlive the app, otherwise buffered log lines are
            // dropped on exit.
            let guard = telemetry::init(&log_dir);
            app.manage(LogGuard(guard));

            app.manage(AppState {
                sessions: Arc::new(SessionManager::new()),
            });

            tracing::info!(version = env!("CARGO_PKG_VERSION"), "harbour starting");
            Ok(())
        })
        .invoke_handler(tauri::generate_handler![
            commands::session::session_open,
            commands::session::session_subscribe,
            commands::session::session_write,
            commands::session::session_resize,
            commands::session::session_ack,
            commands::session::session_set_title,
            commands::session::session_close,
            commands::session::session_list,
            commands::shell::shell_list,
        ])
        .on_window_event(|window, event| {
            // Killing children on window close keeps orphan shells from
            // outliving the app, which Windows in particular does not clean up.
            if let tauri::WindowEvent::Destroyed = event {
                if let Some(state) = window.app_handle().try_state::<AppState>() {
                    state.sessions.close_all();
                }
            }
        })
        .run(tauri::generate_context!())
        .expect("error while running Harbour");
}

/// Held in app state purely so the non-blocking log writer is flushed on
/// shutdown; nothing ever reads it.
struct LogGuard(#[allow(dead_code)] Option<tracing_appender::non_blocking::WorkerGuard>);
