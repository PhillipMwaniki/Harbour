//! Vault IPC surface. See `docs/ipc.md` for the contract.
//!
//! Every store call is wrapped in `spawn_blocking`: SQLite is synchronous, and
//! so is the OS keychain, which on macOS can put an authorisation dialog in
//! front of the user. Neither belongs on the runtime that is also pumping
//! terminal output.

use std::path::PathBuf;
use std::sync::Arc;

use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::session::manager::{self, NewSession};
use crate::session::{SessionClosed, SessionInfo, SessionKind};
use crate::ssh::client::{self, DynAsker, Endpoint};
use crate::ssh::{Asker, HostKeyAnswer, HostKeyQuestion, SecretAnswer, SecretKind, SecretQuestion};
use crate::vault::import::{self, Applied, Candidate, HostKeyCandidate, Preview};
use crate::vault::model::{Folder, FolderId, Host, HostId, HostInput, VaultTree};
use crate::vault::secrets::{self, SecretSlot};
use crate::vault::{ssh_config, xshell};
use crate::xts;
use crate::AppState;

/// Runs a blocking vault operation off the async runtime.
async fn blocking<T, F>(work: F) -> AppResult<T>
where
    T: Send + 'static,
    F: FnOnce() -> AppResult<T> + Send + 'static,
{
    tauri::async_runtime::spawn_blocking(work)
        .await
        .map_err(|err| AppError::internal(format!("vault task failed: {err}")))?
}

// ---------------------------------------------------------------------------
// Reading and editing
// ---------------------------------------------------------------------------

#[tauri::command]
pub async fn vault_tree(state: State<'_, AppState>) -> AppResult<VaultTree> {
    let vault = Arc::clone(&state.vault);
    blocking(move || vault.tree()).await
}

#[tauri::command]
pub async fn vault_create_folder(
    state: State<'_, AppState>,
    parent_id: Option<String>,
    name: String,
) -> AppResult<Folder> {
    let vault = Arc::clone(&state.vault);
    blocking(move || vault.create_folder(parent_id.as_deref(), &name)).await
}

#[tauri::command]
pub async fn vault_rename_folder(
    state: State<'_, AppState>,
    folder_id: FolderId,
    name: String,
) -> AppResult<()> {
    let vault = Arc::clone(&state.vault);
    blocking(move || vault.rename_folder(&folder_id, &name)).await
}

#[tauri::command]
pub async fn vault_move_folder(
    state: State<'_, AppState>,
    folder_id: FolderId,
    parent_id: Option<String>,
) -> AppResult<()> {
    let vault = Arc::clone(&state.vault);
    blocking(move || vault.move_folder(&folder_id, parent_id.as_deref())).await
}

/// Deletes a folder and everything inside it. The UI confirms first; by the
/// time this is called the user has said yes.
#[tauri::command]
pub async fn vault_delete_folder(state: State<'_, AppState>, folder_id: FolderId) -> AppResult<()> {
    let vault = Arc::clone(&state.vault);
    blocking(move || {
        // Take the saved secrets with the hosts, so deleting a folder does not
        // leave passwords behind in the keychain with nothing pointing at them.
        let doomed: Vec<HostId> = vault
            .tree()?
            .hosts
            .into_iter()
            .filter(|host| host.folder_id.as_deref() == Some(folder_id.as_str()))
            .map(|host| host.id)
            .collect();
        vault.delete_folder(&folder_id)?;
        for host in doomed {
            forget_all_secrets(&host);
        }
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn vault_create_host(state: State<'_, AppState>, host: HostInput) -> AppResult<Host> {
    let vault = Arc::clone(&state.vault);
    blocking(move || vault.create_host(host)).await
}

#[tauri::command]
pub async fn vault_update_host(
    state: State<'_, AppState>,
    host_id: HostId,
    host: HostInput,
) -> AppResult<Host> {
    let vault = Arc::clone(&state.vault);
    blocking(move || vault.update_host(&host_id, host)).await
}

#[tauri::command]
pub async fn vault_delete_host(state: State<'_, AppState>, host_id: HostId) -> AppResult<()> {
    let vault = Arc::clone(&state.vault);
    blocking(move || {
        vault.delete_host(&host_id)?;
        forget_all_secrets(&host_id);
        Ok(())
    })
    .await
}

#[tauri::command]
pub async fn vault_move_host(
    state: State<'_, AppState>,
    host_id: HostId,
    folder_id: Option<String>,
) -> AppResult<()> {
    let vault = Arc::clone(&state.vault);
    blocking(move || vault.move_host(&host_id, folder_id.as_deref())).await
}

/// Removes a host's saved password and passphrase from the keychain.
#[tauri::command]
pub async fn vault_forget_secrets(state: State<'_, AppState>, host_id: HostId) -> AppResult<()> {
    let vault = Arc::clone(&state.vault);
    blocking(move || {
        secrets::delete(&host_id, SecretSlot::Password)?;
        secrets::delete(&host_id, SecretSlot::KeyPassphrase)?;
        vault.set_saved_password(&host_id, false)
    })
    .await
}

/// Whether this machine can save secrets at all, so the UI can say so instead
/// of offering a checkbox that will not stick.
#[tauri::command]
pub async fn vault_keychain_available() -> AppResult<bool> {
    blocking(|| Ok(secrets::available())).await
}

/// Best effort: a host is being deleted, so its secrets should go too, but a
/// keychain that refuses must not block the deletion the user asked for.
fn forget_all_secrets(host: &HostId) {
    for slot in [SecretSlot::Password, SecretSlot::KeyPassphrase] {
        if let Err(err) = secrets::delete(host, slot) {
            tracing::warn!(error = %err, "could not remove a saved secret");
        }
    }
}

// ---------------------------------------------------------------------------
// Imports
// ---------------------------------------------------------------------------

/// Reads `~/.ssh/config` - or `path`, if given - and returns what it found.
/// Nothing is written; the UI reviews the list first.
#[tauri::command]
pub async fn vault_preview_ssh_config(path: Option<String>) -> AppResult<Preview> {
    blocking(move || {
        let path = match path {
            Some(path) => PathBuf::from(path),
            None => ssh_config::default_path()
                .ok_or_else(|| AppError::Vault("this machine has no home directory".into()))?,
        };
        let source = path.display().to_string();
        Ok(import::from_ssh_config(ssh_config::read(&path), source))
    })
    .await
}

/// Walks an Xshell export directory, or reads a `.xts` backup. Also writes
/// nothing. A backup also yields the host keys Xshell had accepted, each
/// checked against what Harbour already trusts so the review can say which
/// are new.
#[tauri::command]
pub async fn vault_preview_xshell(state: State<'_, AppState>, path: String) -> AppResult<Preview> {
    let known = Arc::clone(&state.known_hosts);
    blocking(move || {
        let root = PathBuf::from(&path);
        let unreadable =
            |err: std::io::Error| AppError::Vault(format!("could not read {path}: {err}"));

        if xts::Archive::is_archive(&root) {
            let mut archive = xts::Archive::open(&root).map_err(unreadable)?;
            let report = xshell::import_archive(&mut archive).map_err(unreadable)?;
            let keys = archive.host_keys().map_err(unreadable)?;
            let mut preview = import::from_xshell(report, path.clone());
            let (candidates, notes) = import::host_key_candidates(keys, &known);
            preview.host_keys = candidates;
            preview.notes.extend(notes);
            return Ok(preview);
        }

        let report = xshell::import_tree(&root).map_err(unreadable)?;
        Ok(import::from_xshell(report, path))
    })
    .await
}

/// Writes the reviewed candidates into the vault, and the reviewed host keys
/// into Harbour's `known_hosts`.
#[tauri::command]
pub async fn vault_apply_import(
    state: State<'_, AppState>,
    candidates: Vec<Candidate>,
    username: Option<String>,
    host_keys: Option<Vec<HostKeyCandidate>>,
) -> AppResult<Applied> {
    let vault = Arc::clone(&state.vault);
    let known = Arc::clone(&state.known_hosts);
    blocking(move || {
        let mut applied = import::apply(&vault, &candidates, username.as_deref())?;
        applied.host_keys = import::apply_host_keys(&known, &host_keys.unwrap_or_default())?;
        Ok(applied)
    })
    .await
}

// ---------------------------------------------------------------------------
// Connecting
// ---------------------------------------------------------------------------

/// Opens a session to a saved host.
///
/// The only difference from `ssh_connect` is where the answers come from: a
/// saved password is taken from the keychain instead of being asked for, and a
/// password the user chooses to remember is written back.
#[tauri::command]
pub async fn host_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    host_id: HostId,
    cols: u16,
    rows: u16,
) -> AppResult<SessionInfo> {
    let vault = Arc::clone(&state.vault);
    let lookup = host_id.clone();
    let host = blocking(move || vault.host(&lookup)).await?;

    // Follow the jump chain the host sits behind, if any: A -> B -> C means
    // the terminal reaches A by tunnelling through C then B. Every hop is a
    // saved host with its own methods and its own keychain secrets.
    let vault = Arc::clone(&state.vault);
    let dest_id = host.id.clone();
    let chain = blocking(move || resolve_chain(&vault, &dest_id)).await?;

    for hop in &chain {
        if hop.auth.methods().is_empty() {
            return Err(AppError::Vault(format!(
                "{} has no authentication method enabled",
                hop.name
            )));
        }
    }

    let id = manager::new_id();
    let exit_id = id.clone();
    let exit_manager = state.sessions.clone();
    let exit_connections = Arc::clone(&state.connections);
    let exit_transfers = Arc::clone(&state.transfers);
    let exit_edits = Arc::clone(&state.edits);
    let exit_app = app.clone();

    let keychain = secrets::available();
    let make_asker = |host: &Host| -> Arc<dyn DynAsker> {
        Arc::new(SavedHostAsker {
            inner: super::ssh::EventAsker::new(app.clone(), Arc::clone(&state.prompts)),
            host_id: host.id.clone(),
            vault: Arc::clone(&state.vault),
            keychain,
        })
    };
    let endpoint = |host: &Host| Endpoint {
        target: host.target(),
        methods: host.auth.methods(),
        asker: make_asker(host),
    };

    // `chain` is destination-first; the jumps are everything behind it, dialled
    // outermost-first (the first TCP hop leads).
    let dest = endpoint(&host);
    let jumps: Vec<Endpoint> = chain[1..].iter().rev().map(endpoint).collect();

    let title = host.name.clone();
    let connected = client::connect_chain(
        jumps,
        dest,
        Arc::clone(&state.known_hosts),
        cols,
        rows,
        move |reason, code| {
            let closed = SessionClosed::new(exit_id, reason, code);
            exit_manager.remove(&closed.session_id);
            exit_connections.remove(&closed.session_id);
            exit_transfers.cancel_session(&closed.session_id);
            exit_edits.close_session(&closed.session_id);
            if let Err(err) = exit_app.emit("session:closed", &closed) {
                tracing::warn!(error = %err, "failed to emit session:closed");
            }
        },
    )
    .await?;

    tracing::info!(
        session = %id,
        host = %host.id,
        method = connected.method,
        "opened a session to a saved host"
    );

    let opener = connected.transport.opener();
    let info = state.sessions.adopt(NewSession {
        id,
        kind: SessionKind::Ssh,
        title,
        transport: Box::new(connected.transport),
        output: connected.output,
    });
    state.connections.register(info.session_id.clone(), opener);

    let _ = app.emit("session:opened", &info);
    Ok(info)
}

/// The hosts a connection to `host_id` passes through, destination first.
///
/// Follows `jump_host_id` hop by hop. A jump that has been deleted, or a loop,
/// ends the chain rather than failing the connect: a bastion that is gone
/// leaves its dependents merely direct, which is the safe direction to err.
fn resolve_chain(vault: &crate::vault::store::Vault, host_id: &str) -> AppResult<Vec<Host>> {
    let mut chain = vec![vault.host(host_id)?];
    let mut seen = std::collections::HashSet::from([host_id.to_string()]);
    while let Some(jump) = chain.last().and_then(|host| host.jump_host_id.clone()) {
        if !seen.insert(jump.clone()) {
            tracing::warn!(host = %host_id, "jump chain loops; stopping");
            break;
        }
        if chain.len() >= 16 {
            break;
        }
        match vault.host(&jump) {
            Ok(host) => chain.push(host),
            Err(_) => break,
        }
    }
    Ok(chain)
}

/// Answers from the keychain where it can, and from the user where it cannot.
struct SavedHostAsker {
    inner: super::ssh::EventAsker,
    host_id: HostId,
    vault: Arc<crate::vault::store::Vault>,
    /// Whether this machine has a keychain at all, decided once at connect
    /// time rather than per prompt.
    keychain: bool,
}

impl SavedHostAsker {
    fn slot(kind: SecretKind) -> Option<SecretSlot> {
        match kind {
            SecretKind::Password => Some(SecretSlot::Password),
            SecretKind::Passphrase => Some(SecretSlot::KeyPassphrase),
            // Keyboard-interactive prompts are whatever the server decided to
            // ask; a one-time code saved and replayed would be worse than
            // useless, so nothing here is stored.
            SecretKind::Challenge => None,
        }
    }

    async fn saved(&self, slot: SecretSlot) -> Option<String> {
        let host = self.host_id.clone();
        let read = tauri::async_runtime::spawn_blocking(move || secrets::get(&host, slot)).await;
        match read {
            Ok(Ok(secret)) => secret,
            Ok(Err(err)) => {
                // A locked or missing keychain is a reason to ask the user, not
                // a reason to fail the connection.
                tracing::warn!(error = %err, "could not read a saved secret");
                None
            }
            Err(err) => {
                tracing::warn!(error = %err, "the keychain read did not complete");
                None
            }
        }
    }

    async fn remember(&self, slot: SecretSlot, secret: String) {
        let host = self.host_id.clone();
        let vault = Arc::clone(&self.vault);
        let stored = tauri::async_runtime::spawn_blocking(move || {
            secrets::set(&host, slot, &secret)?;
            if slot == SecretSlot::Password {
                vault.set_saved_password(&host, true)?;
            }
            Ok::<_, AppError>(())
        })
        .await;

        match stored {
            Ok(Ok(())) => {}
            Ok(Err(err)) => tracing::warn!(error = %err, "could not save a secret"),
            Err(err) => tracing::warn!(error = %err, "the keychain write did not complete"),
        }
    }
}

impl Asker for SavedHostAsker {
    async fn host_key(&self, question: HostKeyQuestion) -> AppResult<HostKeyAnswer> {
        Asker::host_key(&self.inner, question).await
    }

    async fn secret(&self, mut question: SecretQuestion) -> AppResult<SecretAnswer> {
        let slot = Self::slot(question.kind);

        if let Some(slot) = slot {
            if let Some(secret) = self.saved(slot).await {
                return Ok(SecretAnswer {
                    secret: Some(secret),
                    remember: false,
                });
            }
        }

        // Only offer to remember what there is somewhere to remember.
        question.can_remember = self.keychain && slot.is_some();
        let answer = Asker::secret(&self.inner, question).await?;

        if let (Some(slot), Some(secret), true) = (slot, answer.secret.as_ref(), answer.remember) {
            self.remember(slot, secret.clone()).await;
        }
        Ok(answer)
    }
}
