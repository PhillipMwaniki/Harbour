//! Key generation and deployment IPC. See `docs/ipc.md` for the contract.
//!
//! Two commands: one makes a keypair on disk, the other installs a public key
//! on a saved host. The private key never crosses this boundary - only a path
//! to it and the public half do.

use std::path::PathBuf;
use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, State};

use crate::error::{AppError, AppResult};
use crate::ssh::client;
use crate::ssh::keygen::{self, Installed};
use crate::vault::model::HostId;
use crate::AppState;

async fn blocking<T, F>(work: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| AppError::internal(format!("key task failed: {err}")))?
}

/// A keypair that was just generated. The private key stays on disk at `path`;
/// only its public half travels.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct GeneratedKey {
    pub path: String,
    pub public_path: String,
    pub public_key: String,
    pub fingerprint: String,
}

/// Generates an Ed25519 keypair at `path`, optionally encrypted with
/// `passphrase`. `comment` labels the public key line (default `harbour`).
#[tauri::command]
pub async fn key_generate(
    path: String,
    passphrase: Option<String>,
    comment: Option<String>,
) -> AppResult<GeneratedKey> {
    blocking(move || {
        let comment = comment
            .map(|c| c.trim().to_string())
            .filter(|c| !c.is_empty())
            .unwrap_or_else(|| "harbour".to_string());
        let generated = keygen::generate(&PathBuf::from(path), passphrase.as_deref(), &comment)?;
        Ok(GeneratedKey {
            path: generated.path,
            public_path: generated.public_path,
            public_key: generated.public_key,
            fingerprint: generated.fingerprint,
        })
    })
    .await
}

/// The outcome of installing a key on a host.
#[derive(Debug, Clone, Copy, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct DeployResult {
    /// The key was already in the host's `authorized_keys`; nothing changed.
    pub already_present: bool,
}

/// Installs `public_key` into a saved host's `authorized_keys`.
///
/// Connects the host the same way opening a session does - keychain first, then
/// prompting for a password if needed - and runs an idempotent install command.
/// The frontend attaches the key to the host afterwards, so this only performs
/// the remote install.
#[tauri::command]
pub async fn key_deploy(
    app: AppHandle,
    state: State<'_, AppState>,
    host_id: HostId,
    public_key: String,
) -> AppResult<DeployResult> {
    let command = keygen::install_command(&public_key)?;
    let (dest, jumps) = super::vault::resolve_endpoints(&app, state.inner(), &host_id).await?;
    let outcome =
        client::run_command(jumps, dest, Arc::clone(&state.known_hosts), &command).await?;

    let stdout = String::from_utf8_lossy(&outcome.stdout);
    match keygen::read_marker(&stdout) {
        Ok(Installed::AlreadyPresent) => Ok(DeployResult {
            already_present: true,
        }),
        Ok(Installed::Added) => Ok(DeployResult {
            already_present: false,
        }),
        // No marker means the remote command failed - a read-only home, a full
        // disk. The stderr is the useful part to pass back.
        Err(_) => {
            let stderr = String::from_utf8_lossy(&outcome.stderr);
            Err(AppError::internal(format!(
                "the key could not be installed: {}",
                stderr.trim()
            )))
        }
    }
}
