//! The fleet runner: one command, run on many saved hosts at once.
//!
//! Each host is a full connection - its own jump chain, its own host-key check,
//! its own credentials from the keychain - on which the command is `exec`ed and
//! its output collected. Nothing is interactive: a host whose key is not
//! already trusted, or whose password is not saved and whose agent cannot get
//! in, fails with a message rather than stopping the whole run to ask. That is
//! what makes a run across a hundred hosts something you can start and leave.
//!
//! Results stream back as `fleet:result` events, one per host as it finishes,
//! and the whole set is also returned when every host is done.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::ssh::client::{self, Endpoint};
use crate::ssh::{Asker, HostKeyAnswer, HostKeyQuestion, SecretAnswer, SecretKind, SecretQuestion};
use crate::vault::model::{Host, HostId};
use crate::vault::secrets::{SecretSlot, SecretStore};
use crate::vault::store::Vault;
use crate::AppState;

/// How many hosts to run against at once. Enough to be quick across a big
/// estate, not so many that a shared bastion or the local machine is swamped.
const CONCURRENCY: usize = 8;

/// What running the command on one host came to.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct FleetResult {
    pub host_id: HostId,
    pub name: String,
    /// The command's exit status, when it ran. `null` when it did not.
    pub exit_code: Option<u32>,
    pub stdout: String,
    pub stderr: String,
    /// Set when the host could not be reached, authenticated, or run the
    /// command. When present, the other fields are empty.
    pub error: Option<String>,
}

impl FleetResult {
    fn failed(host_id: HostId, name: String, error: String) -> Self {
        Self {
            host_id,
            name,
            exit_code: None,
            stdout: String::new(),
            stderr: String::new(),
            error: Some(error),
        }
    }
}

/// Runs `command` on each host in `host_ids`, at most [`CONCURRENCY`] at a time.
#[tauri::command]
pub async fn fleet_run(
    app: AppHandle,
    state: State<'_, AppState>,
    host_ids: Vec<HostId>,
    command: String,
) -> AppResult<Vec<FleetResult>> {
    if command.trim().is_empty() {
        return Err(AppError::internal("a command is required"));
    }
    if host_ids.is_empty() {
        return Ok(Vec::new());
    }

    let semaphore = Arc::new(tokio::sync::Semaphore::new(CONCURRENCY));
    let mut tasks = tokio::task::JoinSet::new();

    for host_id in host_ids {
        let vault = Arc::clone(&state.vault);
        let known = Arc::clone(&state.known_hosts);
        let secrets = Arc::clone(&state.secrets);
        let command = command.clone();
        let app = app.clone();
        let semaphore = Arc::clone(&semaphore);

        tasks.spawn(async move {
            // The permit bounds how many connections are open at once.
            let _permit = semaphore.acquire().await;
            let result = run_one(&vault, &known, &secrets, host_id, &command).await;
            // Stream it back the moment it is ready, so a slow host does not
            // hold up the ones already done.
            let _ = app.emit("fleet:result", &result);
            result
        });
    }

    let mut results = Vec::new();
    while let Some(joined) = tasks.join_next().await {
        match joined {
            Ok(result) => results.push(result),
            Err(err) => tracing::warn!(error = %err, "a fleet task did not complete"),
        }
    }
    Ok(results)
}

async fn run_one(
    vault: &Arc<Vault>,
    known: &Arc<crate::ssh::known_hosts::KnownHosts>,
    secrets: &Arc<SecretStore>,
    host_id: HostId,
    command: &str,
) -> FleetResult {
    // The chain the host sits behind, resolved off the async runtime because
    // the vault is synchronous SQLite.
    let chain = {
        let vault = Arc::clone(vault);
        let lookup = host_id.clone();
        match tauri::async_runtime::spawn_blocking(move || {
            super::vault::resolve_chain(&vault, &lookup)
        })
        .await
        {
            Ok(Ok(chain)) => chain,
            Ok(Err(err)) => return FleetResult::failed(host_id.clone(), host_id, err.to_string()),
            Err(err) => {
                return FleetResult::failed(host_id.clone(), host_id, format!("task failed: {err}"))
            }
        }
    };

    let name = chain
        .first()
        .map(|host| host.name.clone())
        .unwrap_or_else(|| host_id.clone());

    for hop in &chain {
        if hop.auth.methods().is_empty() {
            return FleetResult::failed(
                host_id,
                name,
                format!("{} has no authentication method enabled", hop.name),
            );
        }
    }

    let endpoint = |host: &Host| Endpoint {
        target: host.target(),
        methods: host.auth.methods(),
        asker: Arc::new(FleetAsker {
            host_id: host.id.clone(),
            secrets: Arc::clone(secrets),
        }),
    };
    // `chain` is destination-first; the jumps are behind it, dialled outermost
    // first, exactly as an interactive connection builds them.
    let dest = endpoint(&chain[0]);
    let jumps: Vec<Endpoint> = chain[1..].iter().rev().map(endpoint).collect();

    match client::run_command(jumps, dest, Arc::clone(known), command).await {
        Ok(outcome) => FleetResult {
            host_id,
            name,
            exit_code: outcome.exit_code,
            stdout: String::from_utf8_lossy(&outcome.stdout).into_owned(),
            stderr: String::from_utf8_lossy(&outcome.stderr).into_owned(),
            error: None,
        },
        Err(err) => FleetResult::failed(host_id, name, err.to_string()),
    }
}

/// Answers a fleet connection's prompts without ever asking the user: a saved
/// secret if there is one, and nothing otherwise. An unattended run must not
/// block on a dialog, so anything that would need one fails the host instead.
struct FleetAsker {
    host_id: HostId,
    secrets: Arc<SecretStore>,
}

impl Asker for FleetAsker {
    async fn host_key(&self, _question: HostKeyQuestion) -> AppResult<HostKeyAnswer> {
        // A key that is not already trusted is refused: trusting a new one is a
        // decision for a person, at an interactive connection, not for a batch.
        Ok(HostKeyAnswer {
            accept: false,
            remember: false,
        })
    }

    async fn secret(&self, question: SecretQuestion) -> AppResult<SecretAnswer> {
        let slot = match question.kind {
            SecretKind::Password => Some(SecretSlot::Password),
            SecretKind::Passphrase => Some(SecretSlot::KeyPassphrase),
            SecretKind::Challenge => None,
        };

        if let Some(slot) = slot {
            let host = self.host_id.clone();
            let secrets = Arc::clone(&self.secrets);
            let read = tauri::async_runtime::spawn_blocking(move || secrets.get(&host, slot)).await;
            if let Ok(Ok(Some(secret))) = read {
                return Ok(SecretAnswer {
                    secret: Some(secret),
                    remember: false,
                });
            }
        }

        // Nothing saved, and we will not prompt: dismiss, which stops this
        // method and, with nothing else to try, fails the host.
        Ok(SecretAnswer {
            secret: None,
            remember: false,
        })
    }
}
