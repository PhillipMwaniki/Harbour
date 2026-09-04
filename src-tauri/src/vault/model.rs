//! What the vault stores.
//!
//! One rule shapes every type here: **no secrets**. A host record says which
//! authentication methods to try and whether a password is expected to exist;
//! the password itself lives in the OS keychain, addressed by the host's id.
//! See `docs/security.md`.

use serde::{Deserialize, Serialize};

use crate::ssh::{AuthChoice, SshTarget};

pub type HostId = String;
pub type FolderId = String;

/// A node in the session-manager tree.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Folder {
    pub id: FolderId,
    /// `None` for a top-level folder.
    pub parent_id: Option<FolderId>,
    pub name: String,
    pub position: i64,
}

/// How to authenticate to a host, in the order the methods are tried.
///
/// This is the same shape the connect dialog produces, so a saved host and an
/// ad-hoc connection take exactly the same path through the SSH code.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostAuth {
    /// Offer every identity the running SSH agent holds.
    pub use_agent: bool,
    /// A private key file to try. The passphrase, if any, is asked for.
    pub key_path: Option<String>,
    /// Offer password and keyboard-interactive.
    pub use_password: bool,
}

impl Default for HostAuth {
    fn default() -> Self {
        Self {
            use_agent: true,
            key_path: None,
            use_password: true,
        }
    }
}

impl HostAuth {
    /// Turns the flags into the ordered method list the client walks.
    ///
    /// Methods that can succeed without asking the user come first, so a host
    /// reachable by agent never shows a password box.
    pub fn methods(&self) -> Vec<AuthChoice> {
        let mut methods = Vec::new();
        if self.use_agent {
            methods.push(AuthChoice::Agent);
        }
        if let Some(path) = self
            .key_path
            .as_ref()
            .filter(|path| !path.trim().is_empty())
        {
            methods.push(AuthChoice::Key { path: path.clone() });
        }
        if self.use_password {
            methods.push(AuthChoice::Password);
            methods.push(AuthChoice::KeyboardInteractive);
        }
        methods
    }
}

/// A saved host.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Host {
    pub id: HostId,
    pub folder_id: Option<FolderId>,
    /// What the user calls it. Defaults to the hostname, but need not match it.
    pub name: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    pub description: Option<String>,
    pub auth: HostAuth,
    /// Reach this host by tunnelling through another saved host first - a
    /// bastion. `None` for a directly reachable host. A chain is these
    /// pointers followed hop by hop.
    pub jump_host_id: Option<HostId>,
    /// Whether a password for this host is expected in the OS keychain.
    ///
    /// The keychain is authoritative; this is a cache so the UI can offer
    /// "forget password" without a keychain read, which on macOS can raise an
    /// authorisation prompt of its own.
    pub has_saved_password: bool,
    /// Confirm destructive commands before they run on this host - the
    /// production safeguard. Matching happens in the frontend against the
    /// guardrail rules; this only marks the host as one to guard.
    pub guarded: bool,
    pub position: i64,
}

impl Host {
    pub fn target(&self) -> SshTarget {
        SshTarget {
            host: self.hostname.clone(),
            port: self.port,
            user: self.username.clone(),
        }
    }

    /// `user@host`, or `user@host:port` off the default port.
    pub fn label(&self) -> String {
        self.target().label()
    }
}

/// The fields a caller may set. Ids, positions and the keychain flag are the
/// store's business, not the UI's.
#[derive(Debug, Clone, PartialEq, Eq, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostInput {
    pub folder_id: Option<FolderId>,
    pub name: String,
    pub hostname: String,
    pub port: u16,
    pub username: String,
    #[serde(default)]
    pub description: Option<String>,
    #[serde(default)]
    pub auth: HostAuth,
    #[serde(default)]
    pub jump_host_id: Option<HostId>,
    #[serde(default)]
    pub guarded: bool,
}

impl HostInput {
    /// Trims the free-text fields and fills in what can be defaulted, so the
    /// store never holds a host whose name is three spaces.
    pub fn normalised(mut self) -> Self {
        self.hostname = self.hostname.trim().to_string();
        self.username = self.username.trim().to_string();
        self.name = self.name.trim().to_string();
        if self.name.is_empty() {
            self.name = self.hostname.clone();
        }
        self.description = self
            .description
            .map(|text| text.trim().to_string())
            .filter(|text| !text.is_empty());
        self.auth.key_path = self
            .auth
            .key_path
            .map(|path| path.trim().to_string())
            .filter(|path| !path.is_empty());
        if self.port == 0 {
            self.port = 22;
        }
        self.jump_host_id = self
            .jump_host_id
            .map(|id| id.trim().to_string())
            .filter(|id| !id.is_empty());
        self
    }
}

/// Everything the session tree needs, in one round trip. The tree is small -
/// hundreds of hosts at most - so paging it would cost more than it saves.
#[derive(Debug, Clone, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct VaultTree {
    pub folders: Vec<Folder>,
    pub hosts: Vec<Host>,
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn the_default_host_tries_the_agent_before_asking_for_anything() {
        let methods = HostAuth::default().methods();
        assert_eq!(methods.first(), Some(&AuthChoice::Agent));
        assert!(methods.contains(&AuthChoice::Password));
        assert!(methods.contains(&AuthChoice::KeyboardInteractive));
    }

    #[test]
    fn a_key_is_tried_after_the_agent_and_before_a_password() {
        let auth = HostAuth {
            use_agent: true,
            key_path: Some("~/.ssh/id_ed25519".into()),
            use_password: true,
        };
        assert_eq!(
            auth.methods(),
            vec![
                AuthChoice::Agent,
                AuthChoice::Key {
                    path: "~/.ssh/id_ed25519".into()
                },
                AuthChoice::Password,
                AuthChoice::KeyboardInteractive,
            ]
        );
    }

    /// A key path of whitespace is what an empty form field produces; it must
    /// not become an authentication attempt against a file called " ".
    #[test]
    fn a_blank_key_path_is_not_a_method() {
        let auth = HostAuth {
            use_agent: false,
            key_path: Some("   ".into()),
            use_password: false,
        };
        assert!(auth.methods().is_empty());
    }

    #[test]
    fn a_host_with_no_name_is_called_after_its_hostname() {
        let input = HostInput {
            folder_id: None,
            name: "  ".into(),
            hostname: " db.example.com ".into(),
            port: 22,
            username: " deploy ".into(),
            description: Some("   ".into()),
            auth: HostAuth::default(),
            jump_host_id: None,
            guarded: false,
        }
        .normalised();

        assert_eq!(input.name, "db.example.com");
        assert_eq!(input.hostname, "db.example.com");
        assert_eq!(input.username, "deploy");
        assert_eq!(input.description, None);
    }

    #[test]
    fn port_zero_means_the_default() {
        let input = HostInput {
            folder_id: None,
            name: "web".into(),
            hostname: "web.example.com".into(),
            port: 0,
            username: "deploy".into(),
            description: None,
            auth: HostAuth::default(),
            jump_host_id: None,
            guarded: false,
        }
        .normalised();

        assert_eq!(input.port, 22);
    }
}
