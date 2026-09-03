pub mod commands;
pub mod error;
pub mod glob;
pub mod prompt;
pub mod session;
pub mod ssh;
pub mod telemetry;
pub mod vault;

use std::sync::Arc;

use tauri::Manager;

use crate::prompt::Prompts;
use crate::session::manager::SessionManager;
use crate::ssh::known_hosts::KnownHosts;
use crate::vault::store::Vault;

/// Everything the command handlers need. Kept deliberately small: each
/// subsystem (vault, sftp, forwards) adds its own field as it lands.
pub struct AppState {
    pub sessions: Arc<SessionManager>,
    /// Outstanding questions to the user - host keys, credentials.
    pub prompts: Arc<Prompts>,
    /// Host key trust. Reads the user's OpenSSH files, writes only its own.
    pub known_hosts: Arc<KnownHosts>,
    /// Saved hosts and the folder tree. Holds no secrets; those are in the OS
    /// keychain, addressed by host id.
    pub vault: Arc<Vault>,
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

            let config_dir = app
                .path()
                .app_config_dir()
                .unwrap_or_else(|_| std::env::temp_dir().join("harbour"));

            // A vault that will not open must not stop the app from starting:
            // local shells and ad-hoc SSH work without it, and an in-memory
            // stand-in keeps the UI coherent while the user sorts the file out.
            let vault = Vault::open(&config_dir.join("vault.sqlite3"))
                .or_else(|err| {
                    tracing::error!(error = %err, "could not open the vault; continuing without saved hosts");
                    Vault::in_memory()
                })
                .expect("an in-memory vault must always open");

            app.manage(AppState {
                sessions: Arc::new(SessionManager::new()),
                prompts: Prompts::new(),
                // Harbour's own known_hosts sits beside its config, never in
                // ~/.ssh: the user's file is read but never written to.
                known_hosts: Arc::new(KnownHosts::new(config_dir.join("known_hosts"))),
                vault: Arc::new(vault),
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
            commands::ssh::ssh_connect,
            commands::ssh::connection_respond,
            commands::vault::vault_tree,
            commands::vault::vault_create_folder,
            commands::vault::vault_rename_folder,
            commands::vault::vault_move_folder,
            commands::vault::vault_delete_folder,
            commands::vault::vault_create_host,
            commands::vault::vault_update_host,
            commands::vault::vault_delete_host,
            commands::vault::vault_move_host,
            commands::vault::vault_forget_secrets,
            commands::vault::vault_keychain_available,
            commands::vault::vault_preview_ssh_config,
            commands::vault::vault_preview_xshell,
            commands::vault::vault_apply_import,
            commands::vault::host_connect,
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
