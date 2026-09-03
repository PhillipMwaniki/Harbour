pub mod commands;
pub mod error;
pub mod files;
pub mod glob;
pub mod prompt;
pub mod session;
pub mod settings;
pub mod ssh;
pub mod telemetry;
pub mod text;
pub mod transfer;
pub mod vault;
pub mod xts;

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{Emitter, Manager};

use crate::prompt::Prompts;
use crate::session::manager::SessionManager;
use crate::settings::store::SettingsStore;
use crate::ssh::known_hosts::KnownHosts;
use crate::ssh::sftp::Connections;
use crate::transfer::engine::Engine;
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
    /// Preferences: theme, keymap, highlight rules, logging. A separate file
    /// from the vault because it is one people hand-edit and copy about.
    pub settings: Arc<SettingsStore>,
    /// Where session logs go when the user has not named a directory. The
    /// frontend needs it to build a file name, so it is state rather than a
    /// value computed where it is used.
    pub log_dir: PathBuf,
    /// The SSH connection behind each live SSH session, so the file pane can
    /// open SFTP on it. The session manager itself knows nothing about SSH.
    pub connections: Arc<Connections>,
    /// The transfer queue. Every change to a transfer goes out as a
    /// `transfer:update` event carrying the whole transfer.
    pub transfers: Arc<Engine>,
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

            // The engines report through events; they get a handle to emit
            // with and nothing else about the app.
            let transfer_events = app.handle().clone();
            let transfers = Engine::new(Arc::new(move |transfer| {
                let _ = transfer_events.emit("transfer:update", transfer);
            }));

            app.manage(AppState {
                sessions: Arc::new(SessionManager::new()),
                prompts: Prompts::new(),
                // Harbour's own known_hosts sits beside its config, never in
                // ~/.ssh: the user's file is read but never written to.
                known_hosts: Arc::new(KnownHosts::new(config_dir.join("known_hosts"))),
                vault: Arc::new(vault),
                settings: Arc::new(SettingsStore::open(config_dir.join("settings.json"))),
                log_dir: log_dir.clone(),
                connections: Connections::new(),
                transfers,
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
            commands::session::session_log_start,
            commands::session::session_log_stop,
            commands::session::session_log_status,
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
            commands::settings::settings_load,
            commands::settings::settings_save,
            commands::settings::settings_reload,
            commands::settings::settings_paths,
            commands::settings::theme_import,
            commands::settings::highlight_import,
            commands::sftp::sftp_home,
            commands::sftp::sftp_list,
            commands::sftp::sftp_close,
            commands::sftp::local_home,
            commands::sftp::local_roots,
            commands::sftp::local_list,
            commands::sftp::sftp_mkdir,
            commands::sftp::sftp_rename,
            commands::sftp::sftp_remove,
            commands::sftp::local_mkdir,
            commands::sftp::local_rename,
            commands::sftp::local_remove,
            commands::transfer::transfer_enqueue,
            commands::transfer::transfer_list,
            commands::transfer::transfer_pause,
            commands::transfer::transfer_resume,
            commands::transfer::transfer_cancel,
            commands::transfer::transfer_resolve,
            commands::transfer::transfer_remove,
            commands::transfer::transfer_clear_finished,
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
