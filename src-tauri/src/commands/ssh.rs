//! SSH IPC surface. See `docs/ipc.md` for the contract.
//!
//! The interactive parts of connecting - trusting a host key, supplying a
//! password - are event round-trips rather than command arguments: the core
//! asks, the frontend answers with `connection_respond`. That keeps the
//! decision points inside the connection attempt, where they belong, instead
//! of forcing the UI to guess up front what a server will ask for.

use std::sync::Arc;

use serde::Serialize;
use tauri::{AppHandle, Emitter, State};

use crate::error::{AppError, AppResult};
use crate::prompt::Prompts;
use crate::session::manager::{self, NewSession};
use crate::session::{SessionClosed, SessionInfo, SessionKind};
use crate::ssh::client::{self, ConnectRequest};
use crate::ssh::{
    Asker, AuthChoice, HostKeyAnswer, HostKeyQuestion, SecretAnswer, SecretQuestion, SshTarget,
};
use crate::AppState;

/// Opens an SSH session and puts a shell on it.
///
/// Resolves only once the session is live: the round trips for the host key
/// and for credentials happen inside this call, so the frontend has one
/// promise to track and one place to show a failure.
#[tauri::command]
pub async fn ssh_connect(
    app: AppHandle,
    state: State<'_, AppState>,
    target: SshTarget,
    methods: Vec<AuthChoice>,
    cols: u16,
    rows: u16,
) -> AppResult<SessionInfo> {
    if target.host.trim().is_empty() {
        return Err(AppError::internal("a host is required"));
    }
    if target.user.trim().is_empty() {
        return Err(AppError::internal("a username is required"));
    }
    let methods = if methods.is_empty() {
        default_methods()
    } else {
        methods
    };

    let id = manager::new_id();
    let exit_id = id.clone();
    let exit_manager = state.sessions.clone();
    let exit_app = app.clone();

    let asker = Arc::new(EventAsker {
        app: app.clone(),
        prompts: Arc::clone(&state.prompts),
    });

    let label = target.label();
    let connected = client::connect(
        ConnectRequest {
            target,
            methods,
            cols,
            rows,
        },
        asker,
        Arc::clone(&state.known_hosts),
        move |reason, code| {
            let closed = SessionClosed::new(exit_id, reason, code);
            exit_manager.remove(&closed.session_id);
            if let Err(err) = exit_app.emit("session:closed", &closed) {
                tracing::warn!(error = %err, "failed to emit session:closed");
            }
        },
    )
    .await?;

    tracing::info!(
        session = %id,
        fingerprint = %connected.fingerprint,
        method = connected.method,
        "opened ssh session"
    );

    let info = state.sessions.adopt(NewSession {
        id,
        kind: SessionKind::Ssh,
        title: label,
        transport: Box::new(connected.transport),
        output: connected.output,
    });

    let _ = app.emit("session:opened", &info);
    Ok(info)
}

/// Answers a `connection:*_prompt`. The payload shape depends on which prompt
/// is being answered, so it is passed through as-is and validated where it is
/// awaited.
#[tauri::command]
pub async fn connection_respond(
    state: State<'_, AppState>,
    prompt_id: String,
    answer: serde_json::Value,
) -> AppResult<()> {
    state.prompts.answer(&prompt_id, answer)
}

/// What to try when the caller has no preference: the agent first because it
/// needs no interaction, then the two interactive methods. This is the order
/// OpenSSH uses, minus the methods Harbour does not implement.
fn default_methods() -> Vec<AuthChoice> {
    vec![
        AuthChoice::Agent,
        AuthChoice::Password,
        AuthChoice::KeyboardInteractive,
    ]
}

/// Asks the user through the webview, by emitting an event and waiting for the
/// matching `connection_respond`.
struct EventAsker {
    app: AppHandle,
    prompts: Arc<Prompts>,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct HostKeyPrompt<'a> {
    prompt_id: &'a str,
    #[serde(flatten)]
    question: &'a HostKeyQuestion,
}

#[derive(Clone, Serialize)]
#[serde(rename_all = "camelCase")]
struct SecretPrompt<'a> {
    prompt_id: &'a str,
    #[serde(flatten)]
    question: &'a SecretQuestion,
}

impl Asker for EventAsker {
    async fn host_key(&self, question: HostKeyQuestion) -> AppResult<HostKeyAnswer> {
        let pending = self.prompts.begin();
        self.app
            .emit(
                "connection:hostkey_prompt",
                HostKeyPrompt {
                    prompt_id: pending.id(),
                    question: &question,
                },
            )
            .map_err(|err| {
                AppError::internal(format!("could not ask about the host key: {err}"))
            })?;
        pending.answer().await
    }

    async fn secret(&self, question: SecretQuestion) -> AppResult<SecretAnswer> {
        let pending = self.prompts.begin();
        self.app
            .emit(
                "connection:auth_prompt",
                SecretPrompt {
                    prompt_id: pending.id(),
                    question: &question,
                },
            )
            .map_err(|err| AppError::internal(format!("could not ask for credentials: {err}")))?;
        // Nothing here logs, formats or stores the answer: it goes straight
        // back to the authentication attempt that asked for it.
        pending.answer().await
    }
}
