//! Turning other tools' session lists into hosts.
//!
//! Both importers produce the same [`Candidate`], which the UI shows for
//! review before anything is written. Nothing is imported silently: an import
//! that quietly drops a third of someone's estate, or files it under the wrong
//! username, is worse than one that refuses to start.

use serde::{Deserialize, Serialize};

use crate::error::AppResult;
use crate::ssh::known_hosts::{self, KnownHosts, Verdict};
use crate::vault::model::{HostAuth, HostInput};
use crate::vault::ssh_config::ConfigImport;
use crate::vault::store::Vault;
use crate::vault::xshell::{ImportReport, Protocol};
use crate::xts;

/// One host an import found, and whether it can be brought across.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Candidate {
    pub name: String,
    /// Folder path to file it under, mirroring the source's own tree.
    pub folder: Vec<String>,
    pub hostname: String,
    pub port: u16,
    /// `None` when the source did not say, in which case the UI has to ask.
    pub username: Option<String>,
    pub description: Option<String>,
    pub key_path: Option<String>,
    /// The source expected a password. The password itself is never carried
    /// across - see the note in [`crate::vault::xshell`] - so this only means
    /// "ask for one on first connect".
    pub uses_password: bool,
    /// Set when the entry cannot be imported, saying why. The UI shows these
    /// greyed out rather than hiding them, so nothing disappears unexplained.
    pub skip_reason: Option<String>,
}

impl Candidate {
    pub fn importable(&self) -> bool {
        self.skip_reason.is_none()
    }
}

/// How a host key from a backup relates to what Harbour already trusts.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum HostKeyStatus {
    /// Nothing on file for this host and algorithm: importing it saves a
    /// trust-on-first-use prompt later.
    New,
    /// Already trusted, so there is nothing to do.
    Known,
    /// A *different* key of the same algorithm is on file. This is the case
    /// the connect-time prompt defaults to rejecting, and an import must not
    /// be a quieter way past it.
    Changed,
    /// Explicitly revoked here. Never importable.
    Revoked,
}

/// A host key an Xshell backup carried, ready for review.
#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct HostKeyCandidate {
    pub host: String,
    pub port: u16,
    pub algorithm: String,
    /// `SHA256:...`, as the host key prompt shows it.
    pub fingerprint: String,
    /// The key in OpenSSH one-line form, so applying needs no second read of
    /// the archive. A public key is not a secret.
    pub key: String,
    pub status: HostKeyStatus,
}

impl HostKeyCandidate {
    /// Only a key nobody has an opinion about yet may be written.
    pub fn importable(&self) -> bool {
        self.status == HostKeyStatus::New
    }
}

/// Classifies the host keys out of a backup against Harbour's store.
pub fn host_key_candidates(
    found: xts::Found<xts::HostKeyFile>,
    known: &KnownHosts,
) -> (Vec<HostKeyCandidate>, Vec<String>) {
    let mut candidates = Vec::new();
    let mut notes = found.notes;

    for file in found.files {
        let key = match known_hosts::parse_public_key(&file.text) {
            Ok(key) => key,
            Err(reason) => {
                notes.push(format!("key_{}_{}.pub: {reason}", file.host, file.port));
                continue;
            }
        };
        let status = match known.verify(&file.host, file.port, &key) {
            Verdict::Trusted => HostKeyStatus::Known,
            Verdict::Unknown { .. } => HostKeyStatus::New,
            Verdict::Changed { .. } => HostKeyStatus::Changed,
            Verdict::Revoked => HostKeyStatus::Revoked,
        };
        let Ok(encoded) = key.to_openssh() else {
            notes.push(format!(
                "key_{}_{}.pub: the key could not be re-encoded",
                file.host, file.port
            ));
            continue;
        };
        candidates.push(HostKeyCandidate {
            host: file.host,
            port: file.port,
            algorithm: key.algorithm().to_string(),
            fingerprint: known_hosts::fingerprint(&key),
            key: encoded,
            status,
        });
    }

    (candidates, notes)
}

/// What an import found, ready for review.
#[derive(Debug, Clone, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Preview {
    pub candidates: Vec<Candidate>,
    /// Things the user should know that are not about one host: an include
    /// that could not be read, a file that would not parse.
    pub notes: Vec<String>,
    /// Where this was read from, for the dialog's heading.
    pub source: String,
    /// Host keys a `.xts` backup carried, for the same review. Empty for the
    /// other sources.
    #[serde(default)]
    pub host_keys: Vec<HostKeyCandidate>,
}

/// Reads `~/.ssh/config`.
pub fn from_ssh_config(import: ConfigImport, source: String) -> Preview {
    let candidates = import
        .hosts
        .into_iter()
        .map(|host| Candidate {
            // The alias is what the user types today, so it is the name they
            // will look for in the tree.
            name: host.alias,
            folder: Vec::new(),
            hostname: host.hostname,
            port: host.port,
            username: host.user,
            description: None,
            key_path: host.identity_file,
            // `ssh_config` cannot say whether a host takes a password, and
            // guessing "no" would leave the host unconnectable for anyone
            // without a key.
            uses_password: true,
            skip_reason: None,
        })
        .collect();

    Preview {
        candidates,
        notes: import.notes,
        source,
        host_keys: Vec::new(),
    }
}

/// Reads an Xshell export directory.
pub fn from_xshell(report: ImportReport, source: String) -> Preview {
    let candidates = report
        .hosts
        .into_iter()
        .map(|host| {
            // Telnet, serial and the rest are milestone 9. They are still
            // listed, so the user can see what was left behind.
            let skip_reason = match host.protocol {
                Protocol::Ssh | Protocol::Sftp => None,
                other => Some(format!("{other:?} sessions are not supported yet")),
            };
            Candidate {
                name: host.name,
                folder: host.folder,
                hostname: host.hostname,
                port: host.port,
                username: host.username,
                description: host.description,
                // Xshell keeps keys in its own store under a name, not a path,
                // so there is nothing to point at. The user re-selects the key
                // file; the name is preserved as a hint.
                key_path: None,
                uses_password: host.has_stored_password,
                skip_reason,
            }
        })
        .collect();

    let notes = report
        .skipped
        .into_iter()
        .map(|skipped| format!("{}: {}", skipped.path, skipped.reason))
        .collect();

    Preview {
        candidates,
        notes,
        source,
        host_keys: Vec::new(),
    }
}

/// What an import actually did.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct Applied {
    pub hosts: usize,
    pub skipped: usize,
    /// Host keys written to Harbour's `known_hosts`.
    pub host_keys: usize,
}

/// Writes the chosen candidates into the vault, creating folders as needed.
///
/// `fallback_username` fills in for entries whose source did not name one;
/// without it those are skipped rather than imported under a guess.
pub fn apply(
    vault: &Vault,
    candidates: &[Candidate],
    fallback_username: Option<&str>,
) -> AppResult<Applied> {
    let fallback = fallback_username
        .map(str::trim)
        .filter(|name| !name.is_empty());
    let mut applied = Applied::default();

    for candidate in candidates {
        let Some(username) = candidate
            .username
            .as_deref()
            .map(str::trim)
            .filter(|name| !name.is_empty())
            .or(fallback)
        else {
            applied.skipped += 1;
            continue;
        };

        if !candidate.importable() || candidate.hostname.trim().is_empty() {
            applied.skipped += 1;
            continue;
        }

        let folder_id = vault.ensure_folder_path(&candidate.folder)?;
        vault.create_host(HostInput {
            folder_id,
            name: candidate.name.clone(),
            hostname: candidate.hostname.clone(),
            port: candidate.port,
            username: username.to_string(),
            description: candidate.description.clone(),
            auth: HostAuth {
                // An imported host has no history with us, so the agent is
                // worth trying first whatever the source said.
                use_agent: true,
                key_path: candidate.key_path.clone(),
                use_password: candidate.uses_password,
            },
            jump_host_id: None,
        })?;
        applied.hosts += 1;
    }

    Ok(applied)
}

/// Appends the chosen host keys to Harbour's own `known_hosts`.
///
/// Only keys with nothing on file are written, whatever the caller ticked: a
/// key that would replace a trusted one goes through the connect-time prompt,
/// with both fingerprints in front of the user, or not at all.
pub fn apply_host_keys(known: &KnownHosts, keys: &[HostKeyCandidate]) -> AppResult<usize> {
    let mut written = 0;
    for candidate in keys.iter().filter(|candidate| candidate.importable()) {
        let key = known_hosts::parse_public_key(&candidate.key).map_err(|reason| {
            crate::error::AppError::Vault(format!(
                "host key for {}:{} is not valid: {reason}",
                candidate.host, candidate.port
            ))
        })?;
        // The status was computed at preview time; check again so two
        // reviews of the same backup cannot double up a line.
        if !matches!(
            known.verify(&candidate.host, candidate.port, &key),
            Verdict::Unknown { .. }
        ) {
            continue;
        }
        known.learn(&candidate.host, candidate.port, &key)?;
        written += 1;
    }
    Ok(written)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::ssh_config;
    use crate::vault::xshell::{ImportedHost, SkippedFile};

    fn candidate(name: &str, username: Option<&str>) -> Candidate {
        Candidate {
            name: name.into(),
            folder: Vec::new(),
            hostname: format!("{name}.example.com"),
            port: 22,
            username: username.map(str::to_string),
            description: None,
            key_path: None,
            uses_password: true,
            skip_reason: None,
        }
    }

    #[test]
    fn ssh_config_hosts_become_candidates() {
        let import = ssh_config::parse(
            "Host web\n  HostName web.example.com\n  User deploy\n  Port 2222\n  IdentityFile ~/.ssh/id_ed25519\n",
        );
        let preview = from_ssh_config(import, "~/.ssh/config".into());

        assert_eq!(preview.candidates.len(), 1);
        let web = &preview.candidates[0];
        assert_eq!(web.name, "web");
        assert_eq!(web.hostname, "web.example.com");
        assert_eq!(web.port, 2222);
        assert_eq!(web.username.as_deref(), Some("deploy"));
        assert_eq!(web.key_path.as_deref(), Some("~/.ssh/id_ed25519"));
        assert!(web.importable());
    }

    fn xshell_host(name: &str, protocol: Protocol) -> ImportedHost {
        ImportedHost {
            name: name.into(),
            folder: vec!["Production".into()],
            protocol,
            hostname: format!("{name}.example.com"),
            port: 22,
            username: Some("deploy".into()),
            description: None,
            auth_methods: Vec::new(),
            key_name: None,
            encoding: None,
            has_stored_password: true,
        }
    }

    #[test]
    fn xshell_sessions_keep_their_folder_and_password_flag() {
        let report = ImportReport {
            hosts: vec![xshell_host("web", Protocol::Ssh)],
            skipped: Vec::new(),
        };
        let preview = from_xshell(report, "C:/export".into());

        let web = &preview.candidates[0];
        assert_eq!(web.folder, vec!["Production".to_string()]);
        assert!(web.uses_password);
        assert!(web.importable());
    }

    /// A telnet session must still be listed, so the user can see what did not
    /// come across rather than wondering where it went.
    #[test]
    fn unsupported_protocols_are_listed_with_a_reason() {
        let report = ImportReport {
            hosts: vec![xshell_host("legacy", Protocol::Telnet)],
            skipped: Vec::new(),
        };
        let preview = from_xshell(report, "C:/export".into());

        assert_eq!(preview.candidates.len(), 1);
        assert!(!preview.candidates[0].importable());
        assert!(preview.candidates[0]
            .skip_reason
            .as_ref()
            .unwrap()
            .contains("not supported"));
    }

    #[test]
    fn files_that_would_not_parse_become_notes() {
        let report = ImportReport {
            hosts: Vec::new(),
            skipped: vec![SkippedFile {
                path: "C:/export/broken.xsh".into(),
                reason: "no [CONNECTION] section".into(),
            }],
        };
        let preview = from_xshell(report, "C:/export".into());

        assert_eq!(preview.notes.len(), 1);
        assert!(preview.notes[0].contains("broken.xsh"));
    }

    #[test]
    fn applying_writes_hosts_and_mirrors_folders() {
        let vault = Vault::in_memory().unwrap();
        let mut nested = candidate("web", Some("deploy"));
        nested.folder = vec!["Customers".into(), "Acme".into()];

        let applied = apply(&vault, &[nested, candidate("db", Some("root"))], None).unwrap();

        assert_eq!(applied.hosts, 2);
        assert_eq!(applied.skipped, 0);
        let tree = vault.tree().unwrap();
        assert_eq!(tree.hosts.len(), 2);
        assert_eq!(tree.folders.len(), 2);
    }

    #[test]
    fn a_fallback_username_fills_in_for_entries_that_lack_one() {
        let vault = Vault::in_memory().unwrap();

        let applied = apply(&vault, &[candidate("web", None)], Some("deploy")).unwrap();

        assert_eq!(applied.hosts, 1);
        assert_eq!(vault.tree().unwrap().hosts[0].username, "deploy");
    }

    /// Importing under a guessed username would produce hosts that fail to
    /// connect for a reason the user cannot see. Skipping is the honest answer.
    #[test]
    fn an_entry_with_no_username_and_no_fallback_is_skipped() {
        let vault = Vault::in_memory().unwrap();

        let applied = apply(&vault, &[candidate("web", None)], None).unwrap();

        assert_eq!(applied.hosts, 0);
        assert_eq!(applied.skipped, 1);
        assert!(vault.tree().unwrap().hosts.is_empty());
    }

    #[test]
    fn a_blank_fallback_is_not_a_username() {
        let vault = Vault::in_memory().unwrap();

        let applied = apply(&vault, &[candidate("web", None)], Some("   ")).unwrap();

        assert_eq!(applied.skipped, 1);
    }

    #[test]
    fn entries_marked_unimportable_are_not_written() {
        let vault = Vault::in_memory().unwrap();
        let mut blocked = candidate("legacy", Some("deploy"));
        blocked.skip_reason = Some("Telnet sessions are not supported yet".into());

        let applied = apply(&vault, &[blocked, candidate("web", Some("deploy"))], None).unwrap();

        assert_eq!(applied.hosts, 1);
        assert_eq!(applied.skipped, 1);
        assert_eq!(vault.tree().unwrap().hosts[0].name, "web");
    }

    fn test_key(seed: u8) -> russh::keys::PublicKey {
        russh::keys::ssh_key::public::Ed25519PublicKey([seed; 32]).into()
    }

    fn store() -> (KnownHosts, std::path::PathBuf) {
        let dir = std::env::temp_dir().join(format!("harbour-import-kh-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        // Harbour's own file is both read and written here, as it is in the
        // app: what `learn` appends has to count on the next `verify`.
        let path = dir.join("known_hosts");
        (KnownHosts::with_paths(vec![path.clone()], path), dir)
    }

    fn found(entries: &[(&str, u16, &russh::keys::PublicKey)]) -> xts::Found<xts::HostKeyFile> {
        xts::Found {
            files: entries
                .iter()
                .map(|(host, port, key)| xts::HostKeyFile {
                    host: host.to_string(),
                    port: *port,
                    text: key.to_openssh().unwrap(),
                })
                .collect(),
            notes: Vec::new(),
        }
    }

    #[test]
    fn host_keys_are_classified_against_the_store() {
        let (known, dir) = store();
        let trusted = test_key(1);
        known.learn("web.example.com", 22, &trusted).unwrap();

        let (candidates, notes) = host_key_candidates(
            found(&[
                ("web.example.com", 22, &trusted),
                ("web.example.com", 2222, &trusted),
                ("db.example.com", 22, &test_key(2)),
                ("web.example.com", 22, &test_key(3)),
            ]),
            &known,
        );
        std::fs::remove_dir_all(dir).ok();

        assert!(notes.is_empty(), "{notes:?}");
        let statuses: Vec<_> = candidates.iter().map(|c| c.status).collect();
        assert_eq!(
            statuses,
            [
                HostKeyStatus::Known,
                // A different port is a different host as far as trust goes.
                HostKeyStatus::New,
                HostKeyStatus::New,
                HostKeyStatus::Changed,
            ]
        );
        assert_eq!(candidates[0].algorithm, "ssh-ed25519");
        assert!(candidates[0].fingerprint.starts_with("SHA256:"));
    }

    #[test]
    fn a_key_that_will_not_parse_becomes_a_note() {
        let (known, dir) = store();
        let mut files = found(&[("web", 22, &test_key(1))]);
        files.files[0].text =
            "---- BEGIN SSH2 PUBLIC KEY ----\nnot base64!!\n---- END SSH2 PUBLIC KEY ----\n".into();

        let (candidates, notes) = host_key_candidates(files, &known);
        std::fs::remove_dir_all(dir).ok();

        assert!(candidates.is_empty());
        assert_eq!(notes.len(), 1);
        assert!(notes[0].starts_with("key_web_22.pub:"));
    }

    /// Only new keys are written. A changed key ticked in a hurry must not
    /// become a second trusted line for the host.
    #[test]
    fn applying_writes_new_keys_and_nothing_else() {
        let (known, dir) = store();
        let trusted = test_key(1);
        known.learn("web.example.com", 22, &trusted).unwrap();

        let (candidates, _) = host_key_candidates(
            found(&[
                ("web.example.com", 22, &trusted),
                ("db.example.com", 22, &test_key(2)),
                ("web.example.com", 22, &test_key(3)),
            ]),
            &known,
        );

        let written = apply_host_keys(&known, &candidates).unwrap();
        assert_eq!(written, 1);
        assert_eq!(
            known.verify("db.example.com", 22, &test_key(2)),
            Verdict::Trusted
        );
        assert!(matches!(
            known.verify("web.example.com", 22, &test_key(3)),
            Verdict::Changed { .. }
        ));

        // Applying the same review twice does not duplicate the line.
        assert_eq!(apply_host_keys(&known, &candidates).unwrap(), 0);
        let file = std::fs::read_to_string(known.write_path()).unwrap();
        assert_eq!(file.lines().count(), 2);
        std::fs::remove_dir_all(dir).ok();
    }

    /// Importing the same export twice should not multiply the folder tree.
    #[test]
    fn importing_twice_reuses_the_folders_it_made() {
        let vault = Vault::in_memory().unwrap();
        let mut nested = candidate("web", Some("deploy"));
        nested.folder = vec!["Production".into()];

        apply(&vault, std::slice::from_ref(&nested), None).unwrap();
        apply(&vault, &[nested], None).unwrap();

        let tree = vault.tree().unwrap();
        assert_eq!(tree.folders.len(), 1);
        assert_eq!(tree.hosts.len(), 2, "the hosts themselves are not deduped");
    }
}
