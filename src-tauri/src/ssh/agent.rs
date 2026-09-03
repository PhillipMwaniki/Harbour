//! Finding the running SSH agent.
//!
//! There is no single answer across platforms: Unix has a socket named by
//! `SSH_AUTH_SOCK`, Windows has either the OpenSSH agent on a named pipe or
//! Pageant on its own window-message transport. The rest of the SSH code only
//! wants "an agent, if there is one", so the platform detail stops here.

use russh::keys::agent::client::{AgentClient, AgentStream};

use crate::error::{AppError, AppResult};

/// An agent connection with its transport erased, so callers do not have to be
/// generic over which kind of agent answered.
pub type Agent = AgentClient<Box<dyn AgentStream + Send + Unpin>>;

/// The OpenSSH for Windows agent always listens here.
#[cfg(windows)]
const OPENSSH_PIPE: &str = r"\\.\pipe\openssh-ssh-agent";

/// Connects to whichever agent this machine has.
#[cfg(unix)]
pub async fn connect() -> AppResult<Agent> {
    AgentClient::connect_env()
        .await
        .map(AgentClient::dynamic)
        .map_err(|err| AppError::SshAgent(format!("{err} (is SSH_AUTH_SOCK set?)")))
}

/// Connects to whichever agent this machine has.
///
/// `SSH_AUTH_SOCK` wins when it is set - Git for Windows and WSL interop both
/// use it, and it may name a pipe - then the stock OpenSSH pipe, then Pageant,
/// which is what PuTTY and its descendants speak.
#[cfg(windows)]
pub async fn connect() -> AppResult<Agent> {
    let mut attempts: Vec<(String, String)> = Vec::new();

    if let Ok(sock) = std::env::var("SSH_AUTH_SOCK") {
        if !sock.trim().is_empty() {
            match AgentClient::connect_named_pipe(&sock).await {
                Ok(agent) => return Ok(agent.dynamic()),
                Err(err) => attempts.push((format!("SSH_AUTH_SOCK ({sock})"), err.to_string())),
            }
        }
    }

    match AgentClient::connect_named_pipe(OPENSSH_PIPE).await {
        Ok(agent) => return Ok(agent.dynamic()),
        Err(err) => attempts.push(("the OpenSSH agent pipe".into(), err.to_string())),
    }

    match AgentClient::connect_pageant().await {
        Ok(agent) => return Ok(agent.dynamic()),
        Err(err) => attempts.push(("Pageant".into(), err.to_string())),
    }

    let detail = attempts
        .into_iter()
        .map(|(what, err)| format!("{what}: {err}"))
        .collect::<Vec<_>>()
        .join("; ");
    Err(AppError::SshAgent(detail))
}
