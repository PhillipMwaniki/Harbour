use crate::error::AppResult;
use crate::session::shell::{self, ShellSpec};

/// Lists local shells, best default first. Enumeration touches the filesystem
/// and shells out to `wsl.exe`, so it runs off the UI thread.
#[tauri::command]
pub async fn shell_list() -> AppResult<Vec<ShellSpec>> {
    tauri::async_runtime::spawn_blocking(shell::detect)
        .await
        .map_err(|err| crate::error::AppError::internal(err.to_string()))
}
