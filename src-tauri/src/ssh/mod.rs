//! SSH transport: connecting, authenticating, and running a remote shell.
//!
//! The split here mirrors the one in `session`: [`client`] establishes a
//! connection and hands back something the session manager can own, while
//! [`transport`] is the running channel. [`known_hosts`] answers the trust
//! question and nothing else.
//!
//! Everything that needs a human - trusting a host key, a password, a key
//! passphrase - goes through [`Asker`]. The core never reads a secret from
//! anywhere but the user (milestone 3 adds the vault as a second source), and
//! never holds one longer than the authentication attempt that uses it.

pub mod agent;
pub mod client;
pub mod forward;
pub mod known_hosts;
pub mod sftp;
pub mod transport;

use std::future::Future;

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::ssh::known_hosts::StoredKey;

/// Where to connect and as whom.
#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SshTarget {
    pub host: String,
    pub port: u16,
    pub user: String,
}

impl SshTarget {
    /// `user@host`, or `user@host:port` when the port is not the default. Used
    /// as the tab title and in every message the user sees.
    pub fn label(&self) -> String {
        if self.port == 22 {
            format!("{}@{}", self.user, self.host)
        } else {
            format!("{}@{}:{}", self.user, self.host, self.port)
        }
    }
}

/// One authentication method to try, in the order the caller listed them.
///
/// This mirrors OpenSSH's `PreferredAuthentications`: the client proposes,
/// the server accepts or refuses, and we move down the list. Milestone 3
/// fills these in from the vault instead of from the connect dialog.
#[derive(Debug, Clone, Deserialize, PartialEq, Eq)]
#[serde(tag = "kind", rename_all = "camelCase")]
pub enum AuthChoice {
    /// Every identity the running SSH agent will offer.
    Agent,
    /// A private key file. The passphrase, if the key needs one, is asked for.
    Key { path: String },
    /// `password` auth, asked for when the method is reached.
    Password,
    /// `keyboard-interactive`: the server sends the prompts, we relay them.
    /// This is how many servers actually implement password login.
    KeyboardInteractive,
}

impl AuthChoice {
    pub fn describe(&self) -> &'static str {
        match self {
            AuthChoice::Agent => "agent",
            AuthChoice::Key { .. } => "publickey",
            AuthChoice::Password => "password",
            AuthChoice::KeyboardInteractive => "keyboard-interactive",
        }
    }
}

/// What the store had to say, in the terms the prompt is phrased in.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum HostKeyStatus {
    /// Nothing on file for this host: trust on first use.
    Unknown,
    /// A different key of the same type is on file. Defaults to reject.
    Changed,
}

/// Everything the user needs in order to answer "do you trust this host?".
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyQuestion {
    pub host: String,
    pub port: u16,
    pub status: HostKeyStatus,
    /// SSH name of the offered key's algorithm, e.g. `ssh-ed25519`.
    pub algorithm: String,
    /// `SHA256:...` for the key the server just offered.
    pub fingerprint: String,
    /// What is already on file for this host: the key that changed, or keys of
    /// other types. Empty for a host never seen before.
    pub stored: Vec<StoredKey>,
}

#[derive(Debug, Clone, Copy, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyAnswer {
    /// False - the default for a changed key - aborts the connection.
    pub accept: bool,
    /// Write the key to Harbour's `known_hosts` so the next connection is
    /// silent. Accepting without remembering is a one-shot trust.
    pub remember: bool,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub enum SecretKind {
    /// The account password on the remote host.
    Password,
    /// The passphrase protecting a local private key file.
    Passphrase,
    /// A prompt the server itself worded, under keyboard-interactive.
    Challenge,
}

/// A request for one secret. `label` is already phrased for display; nothing
/// downstream should have to construct sentences.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretQuestion {
    pub host: String,
    pub user: String,
    pub kind: SecretKind,
    pub label: String,
    /// Server-supplied context under keyboard-interactive; empty otherwise.
    pub instruction: String,
    /// Whether the typed answer may be shown. False for every real secret;
    /// keyboard-interactive can ask non-secret questions too.
    pub echo: bool,
    /// Whether there is anywhere to save this answer. False for an ad-hoc
    /// connection, which has no host record to attach a keychain entry to, and
    /// false when the machine has no usable keychain - offering to remember
    /// something that will be forgotten is worse than not offering.
    pub can_remember: bool,
}

#[derive(Debug, Clone, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SecretAnswer {
    /// `None` means the user dismissed the prompt: stop, do not try the next
    /// method, and do not treat it as an authentication failure.
    pub secret: Option<String>,
    /// Save it in the OS keychain. Only ever set when the question said it
    /// could be.
    #[serde(default)]
    pub remember: bool,
}

/// How the SSH code reaches the user.
///
/// An interface rather than a direct call to Tauri, so the connection logic
/// can be exercised against a scripted answerer in tests - and so a future
/// non-interactive path (the fleet runner in milestone 9) can refuse to
/// prompt instead of hanging.
pub trait Asker: Send + Sync + 'static {
    fn host_key(
        &self,
        question: HostKeyQuestion,
    ) -> impl Future<Output = AppResult<HostKeyAnswer>> + Send;

    fn secret(
        &self,
        question: SecretQuestion,
    ) -> impl Future<Output = AppResult<SecretAnswer>> + Send;
}
