//! Settings and colour scheme IPC surface. See `docs/ipc.md` for the contract.
//!
//! File work runs on the blocking pool for the same reason the vault's does:
//! the runtime underneath these handlers is also pumping terminal output, and
//! a scheme import can be a directory of two hundred files on a network share.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::State;

use crate::error::{AppError, AppResult};
use crate::settings::highlight::{self, HighlightImport};
use crate::settings::scheme::{self, SchemeImport};
use crate::settings::Settings;
use crate::AppState;

async fn blocking<T, F>(work: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| AppError::internal(format!("settings task failed: {err}")))?
}

/// The settings as last read. Cheap: the document is already in memory.
#[tauri::command]
pub async fn settings_load(state: State<'_, AppState>) -> AppResult<Settings> {
    Ok(state.settings.get())
}

/// Replaces the whole document. There is no partial update on purpose - see
/// [`crate::settings::store::SettingsStore::replace`].
#[tauri::command]
pub async fn settings_save(state: State<'_, AppState>, settings: Settings) -> AppResult<Settings> {
    let store = Arc::clone(&state.settings);
    blocking(move || store.replace(settings)).await
}

/// Re-reads the file from disk, for when it was edited outside Harbour.
#[tauri::command]
pub async fn settings_reload(state: State<'_, AppState>) -> AppResult<Settings> {
    let store = Arc::clone(&state.settings);
    blocking(move || Ok(store.reload())).await
}

/// The directories and files behind the settings, so the settings dialog can
/// point at them and the frontend can name a log file without guessing.
#[derive(Debug, Clone, serde::Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SettingsPaths {
    pub settings: String,
    pub logs: String,
}

#[tauri::command]
pub async fn settings_paths(state: State<'_, AppState>) -> AppResult<SettingsPaths> {
    Ok(SettingsPaths {
        settings: state.settings.path().display().to_string(),
        logs: state.log_dir.display().to_string(),
    })
}

/// Reads colour schemes from a file or a directory. Writes nothing: the
/// caller reviews what came back and saves the ones it wants.
#[tauri::command]
pub async fn theme_import(path: String) -> AppResult<SchemeImport> {
    let path = PathBuf::from(shellexpand(&path));
    blocking(move || scheme::import(&path)).await
}

/// Reads Xshell highlight sets - a `.hls`, a directory of them, or the ones
/// inside a `.xts` backup - as highlight rules. Writes nothing.
#[tauri::command]
pub async fn highlight_import(path: String) -> AppResult<HighlightImport> {
    let path = PathBuf::from(shellexpand(&path));
    blocking(move || highlight::import(&path)).await
}

/// Expands a leading `~`, which people type and no filesystem understands.
fn shellexpand(path: &str) -> String {
    let trimmed = path.trim();
    let Some(rest) = trimmed.strip_prefix('~') else {
        return trimmed.to_string();
    };
    if !(rest.is_empty() || rest.starts_with('/') || rest.starts_with('\\')) {
        // `~user` is somebody else's home directory, which is not ours to
        // guess at.
        return trimmed.to_string();
    }
    match dirs::home_dir() {
        Some(home) => format!("{}{}", home.display(), rest),
        None => trimmed.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn expands_a_leading_tilde_only() {
        let home = dirs::home_dir().unwrap().display().to_string();
        assert_eq!(shellexpand("~/schemes"), format!("{home}/schemes"));
        assert_eq!(shellexpand("  ~  ").trim(), home);
        assert_eq!(shellexpand("~other/schemes"), "~other/schemes");
        assert_eq!(shellexpand("/etc/schemes"), "/etc/schemes");
    }
}
