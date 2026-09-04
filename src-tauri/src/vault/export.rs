//! Encrypted, portable export and import of a whole vault.
//!
//! A vault is a SQLite file and a scatter of keychain entries: perfectly good
//! on the machine that owns them, useless to carry to another. This turns the
//! two into one sealed blob and back.
//!
//! The plaintext is a small JSON document - the folder tree, the hosts, and,
//! if the user asks, their secrets - and it is sealed with [`crypto::seal`], so
//! a passphrase is the only thing not in the file. Nothing here writes an
//! unencrypted secret anywhere: secrets live in the document only for the
//! instant between reading them from the keychain and sealing them, and on
//! import they go straight back into the keychain, never to disk.
//!
//! Import is deliberately additive. Every id in the document is reissued as it
//! lands, so importing into a vault that already has entries appends a fresh
//! copy of the tree rather than overwriting anything - a merge, never a clobber.
//! Parent, jump-host and secret references are rewritten to the new ids as they
//! are assigned.

use std::collections::HashMap;

use serde::{Deserialize, Serialize};

use crate::crypto;
use crate::error::{AppError, AppResult};
use crate::vault::model::{FolderId, HostAuth, HostId, HostInput};
use crate::vault::secrets::SecretSlot;
use crate::vault::store::Vault;

/// Names the document so a decrypted blob that is not one of ours is caught
/// with a clear message rather than a serde error.
const FORMAT: &str = "harbour-vault-export";
/// The document schema. Bumped only if the JSON shape changes; the crypto
/// envelope carries its own version for the sealing layer.
const DOC_VERSION: u32 = 1;

/// What to put in an export.
#[derive(Debug, Clone, Copy)]
pub struct ExportOptions {
    /// Include the passwords and key passphrases the keychain holds. Off by
    /// default: an export without secrets is a shareable list of hosts, one
    /// with them is a full credential backup, and the difference should be a
    /// deliberate choice.
    pub include_secrets: bool,
}

/// What an import added. Everything here is new; nothing was replaced.
#[derive(Debug, Clone, Copy, Default, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportSummary {
    pub folders: usize,
    pub hosts: usize,
    pub secrets: usize,
}

// ---------------------------------------------------------------------------
// The on-disk document
// ---------------------------------------------------------------------------

/// The sealed plaintext. Dedicated types, not the store's own structs, so the
/// file format is decided here and does not drift when an internal type gains
/// a field.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportDoc {
    format: String,
    version: u32,
    /// Unix seconds, matching the rest of the codebase; informational only.
    exported_at: i64,
    folders: Vec<ExportFolder>,
    hosts: Vec<ExportHost>,
    /// Empty unless secrets were requested.
    #[serde(default)]
    secrets: Vec<ExportSecret>,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportFolder {
    id: String,
    parent_id: Option<String>,
    name: String,
    position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportHost {
    id: String,
    folder_id: Option<String>,
    name: String,
    hostname: String,
    port: u16,
    username: String,
    #[serde(default)]
    description: Option<String>,
    auth: HostAuth,
    #[serde(default)]
    jump_host_id: Option<String>,
    position: i64,
}

#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
struct ExportSecret {
    host_id: String,
    #[serde(default)]
    password: Option<String>,
    #[serde(default)]
    passphrase: Option<String>,
}

fn now() -> i64 {
    std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs() as i64)
        .unwrap_or(0)
}

// ---------------------------------------------------------------------------
// Export
// ---------------------------------------------------------------------------

/// Reads the vault (and, if asked, the keychain via `read_secret`), then seals
/// the lot under `passphrase`. The returned bytes are the file to write.
pub fn export_sealed<R>(
    vault: &Vault,
    passphrase: &str,
    opts: ExportOptions,
    read_secret: R,
) -> AppResult<Vec<u8>>
where
    R: Fn(&HostId, SecretSlot) -> Option<String>,
{
    if passphrase.is_empty() {
        return Err(AppError::Crypto("an export needs a passphrase".into()));
    }
    let doc = build_document(vault, opts, read_secret)?;
    let json = serde_json::to_vec(&doc)
        .map_err(|err| AppError::internal(format!("could not serialise the export: {err}")))?;
    crypto::seal(passphrase, &json)
}

fn build_document<R>(vault: &Vault, opts: ExportOptions, read_secret: R) -> AppResult<ExportDoc>
where
    R: Fn(&HostId, SecretSlot) -> Option<String>,
{
    let tree = vault.tree()?;

    let folders = tree
        .folders
        .iter()
        .map(|f| ExportFolder {
            id: f.id.clone(),
            parent_id: f.parent_id.clone(),
            name: f.name.clone(),
            position: f.position,
        })
        .collect();

    let hosts = tree
        .hosts
        .iter()
        .map(|h| ExportHost {
            id: h.id.clone(),
            folder_id: h.folder_id.clone(),
            name: h.name.clone(),
            hostname: h.hostname.clone(),
            port: h.port,
            username: h.username.clone(),
            description: h.description.clone(),
            auth: h.auth.clone(),
            jump_host_id: h.jump_host_id.clone(),
            position: h.position,
        })
        .collect();

    let secrets = if opts.include_secrets {
        tree.hosts
            .iter()
            .filter_map(|h| {
                let password = read_secret(&h.id, SecretSlot::Password);
                let passphrase = read_secret(&h.id, SecretSlot::KeyPassphrase);
                (password.is_some() || passphrase.is_some()).then(|| ExportSecret {
                    host_id: h.id.clone(),
                    password,
                    passphrase,
                })
            })
            .collect()
    } else {
        Vec::new()
    };

    Ok(ExportDoc {
        format: FORMAT.to_string(),
        version: DOC_VERSION,
        exported_at: now(),
        folders,
        hosts,
        secrets,
    })
}

// ---------------------------------------------------------------------------
// Import
// ---------------------------------------------------------------------------

/// Opens a sealed export, parses it, and merges it into `vault`. Secrets are
/// handed to `write_secret` - in production, the keychain - one at a time; they
/// are never written anywhere else. Returns what was added.
pub fn import_sealed<W>(
    vault: &Vault,
    passphrase: &str,
    bytes: &[u8],
    write_secret: W,
) -> AppResult<ImportSummary>
where
    W: FnMut(&HostId, SecretSlot, &str) -> AppResult<()>,
{
    let doc = open_document(bytes, passphrase)?;
    merge_document(vault, &doc, write_secret)
}

fn open_document(bytes: &[u8], passphrase: &str) -> AppResult<ExportDoc> {
    let json = crypto::open(passphrase, bytes)?;
    let doc: ExportDoc = serde_json::from_slice(&json)
        .map_err(|_| AppError::Crypto("this file is not a Harbour export".into()))?;
    if doc.format != FORMAT {
        return Err(AppError::Crypto("this file is not a Harbour export".into()));
    }
    if doc.version != DOC_VERSION {
        return Err(AppError::Crypto(format!(
            "this export is version {}; this Harbour reads version {DOC_VERSION}",
            doc.version
        )));
    }
    Ok(doc)
}

fn merge_document<W>(
    vault: &Vault,
    doc: &ExportDoc,
    mut write_secret: W,
) -> AppResult<ImportSummary>
where
    W: FnMut(&HostId, SecretSlot, &str) -> AppResult<()>,
{
    let folder_map = create_folders(vault, &doc.folders)?;
    let host_map = create_hosts(vault, &doc.hosts, &folder_map)?;
    let secrets = restore_secrets(vault, &doc.secrets, &host_map, &mut write_secret)?;

    Ok(ImportSummary {
        folders: folder_map.len(),
        hosts: host_map.len(),
        secrets,
    })
}

/// Recreates the folder tree, parent before child, returning old id -> new id.
///
/// A folder is created once its parent has been (or once we know its parent is
/// not in the set, which makes it a top-level folder here). A document with a
/// cycle - which a well-formed export cannot have - would stall the walk, so
/// anything still unplaced after no further progress is attached at the top
/// level rather than dropped.
fn create_folders(vault: &Vault, folders: &[ExportFolder]) -> AppResult<HashMap<String, FolderId>> {
    let mut map: HashMap<String, FolderId> = HashMap::new();
    let mut pending: Vec<&ExportFolder> = folders.iter().collect();
    pending.sort_by_key(|f| f.position);

    while !pending.is_empty() {
        let mut progressed = false;
        let mut still = Vec::new();
        for folder in pending {
            let parent_in_set = folder
                .parent_id
                .as_ref()
                .is_some_and(|p| folders.iter().any(|g| &g.id == p));
            let ready = match &folder.parent_id {
                Some(p) if parent_in_set => map.contains_key(p),
                _ => true,
            };
            if ready {
                let new_parent = folder.parent_id.as_ref().and_then(|p| map.get(p)).cloned();
                let created = vault.create_folder(new_parent.as_deref(), &folder.name)?;
                map.insert(folder.id.clone(), created.id);
                progressed = true;
            } else {
                still.push(folder);
            }
        }
        pending = still;
        if !progressed {
            // No parent will ever arrive; attach the rest at the top level.
            for folder in pending.drain(..) {
                let created = vault.create_folder(None, &folder.name)?;
                map.insert(folder.id.clone(), created.id);
            }
        }
    }
    Ok(map)
}

/// Recreates the hosts, returning old id -> new id. Jumps are attached in a
/// second pass, because a host's jump target may be created after it.
fn create_hosts(
    vault: &Vault,
    hosts: &[ExportHost],
    folder_map: &HashMap<String, FolderId>,
) -> AppResult<HashMap<String, HostId>> {
    let mut ordered: Vec<&ExportHost> = hosts.iter().collect();
    ordered.sort_by_key(|h| h.position);

    let mut map: HashMap<String, HostId> = HashMap::new();
    for host in &ordered {
        let folder = host
            .folder_id
            .as_ref()
            .and_then(|f| folder_map.get(f))
            .cloned();
        let created = vault.create_host(host_input(host, folder, None))?;
        map.insert(host.id.clone(), created.id);
    }

    for host in &ordered {
        // Only jumps that point inside the imported set are reconnected; a jump
        // to a host that was not exported is dropped, leaving that host direct -
        // the same safe direction the connect path errs in.
        let (Some(old_jump), Some(new_id)) = (&host.jump_host_id, map.get(&host.id)) else {
            continue;
        };
        let Some(new_jump) = map.get(old_jump) else {
            continue;
        };
        let folder = host
            .folder_id
            .as_ref()
            .and_then(|f| folder_map.get(f))
            .cloned();
        vault.update_host(new_id, host_input(host, folder, Some(new_jump.clone())))?;
    }
    Ok(map)
}

fn host_input(host: &ExportHost, folder: Option<FolderId>, jump: Option<HostId>) -> HostInput {
    HostInput {
        folder_id: folder,
        name: host.name.clone(),
        hostname: host.hostname.clone(),
        port: host.port,
        username: host.username.clone(),
        description: host.description.clone(),
        auth: host.auth.clone(),
        jump_host_id: jump,
    }
}

fn restore_secrets<W>(
    vault: &Vault,
    secrets: &[ExportSecret],
    host_map: &HashMap<String, HostId>,
    write_secret: &mut W,
) -> AppResult<usize>
where
    W: FnMut(&HostId, SecretSlot, &str) -> AppResult<()>,
{
    let mut restored = 0;
    for secret in secrets {
        let Some(new_id) = host_map.get(&secret.host_id) else {
            continue;
        };
        if let Some(password) = &secret.password {
            write_secret(new_id, SecretSlot::Password, password)?;
            // Mirror the cache the connect path reads before touching the
            // keychain, so the imported host shows as remembering its password.
            vault.set_saved_password(new_id, true)?;
            restored += 1;
        }
        if let Some(passphrase) = &secret.passphrase {
            write_secret(new_id, SecretSlot::KeyPassphrase, passphrase)?;
            restored += 1;
        }
    }
    Ok(restored)
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::vault::model::HostInput;
    use std::collections::HashMap;

    fn host_input(name: &str, folder: Option<&str>) -> HostInput {
        HostInput {
            folder_id: folder.map(str::to_string),
            name: name.into(),
            hostname: format!("{name}.example.com"),
            port: 22,
            username: "deploy".into(),
            description: None,
            auth: HostAuth::default(),
            jump_host_id: None,
        }
    }

    /// A vault with a small tree: a folder, a host in it, a top-level bastion,
    /// and an internal host that jumps through the bastion.
    fn seeded_vault() -> Vault {
        let vault = Vault::in_memory().unwrap();
        let prod = vault.create_folder(None, "Production").unwrap();
        vault
            .create_host(host_input("web", Some(&prod.id)))
            .unwrap();
        let bastion = vault.create_host(host_input("bastion", None)).unwrap();
        let mut internal = host_input("internal", None);
        internal.jump_host_id = Some(bastion.id.clone());
        vault.create_host(internal).unwrap();
        vault
    }

    #[test]
    fn a_vault_round_trips_through_a_sealed_export() {
        let source = seeded_vault();
        let sealed = export_sealed(
            &source,
            "correct horse",
            ExportOptions {
                include_secrets: false,
            },
            |_, _| None,
        )
        .unwrap();
        assert!(crypto::is_envelope(&sealed));

        let target = Vault::in_memory().unwrap();
        let summary = import_sealed(&target, "correct horse", &sealed, |_, _, _| Ok(())).unwrap();
        assert_eq!(summary.folders, 1);
        assert_eq!(summary.hosts, 3);

        let tree = target.tree().unwrap();
        assert_eq!(tree.folders.len(), 1);
        assert_eq!(tree.folders[0].name, "Production");

        // The web host is filed under the recreated folder...
        let web = tree.hosts.iter().find(|h| h.name == "web").unwrap();
        assert_eq!(web.folder_id.as_deref(), Some(tree.folders[0].id.as_str()));

        // ...and the jump is rewired to the bastion's *new* id, not the old one.
        let bastion = tree.hosts.iter().find(|h| h.name == "bastion").unwrap();
        let internal = tree.hosts.iter().find(|h| h.name == "internal").unwrap();
        assert_eq!(internal.jump_host_id.as_deref(), Some(bastion.id.as_str()));
    }

    #[test]
    fn importing_appends_rather_than_replacing() {
        let source = seeded_vault();
        let sealed = export_sealed(
            &source,
            "pw",
            ExportOptions {
                include_secrets: false,
            },
            |_, _| None,
        )
        .unwrap();

        // A target that already holds its own host.
        let target = Vault::in_memory().unwrap();
        target.create_host(host_input("existing", None)).unwrap();

        import_sealed(&target, "pw", &sealed, |_, _, _| Ok(())).unwrap();
        // The original survives and the three imported hosts join it.
        let tree = target.tree().unwrap();
        assert_eq!(tree.hosts.len(), 4);
        assert!(tree.hosts.iter().any(|h| h.name == "existing"));
    }

    #[test]
    fn a_second_import_of_the_same_file_duplicates_rather_than_colliding() {
        let source = seeded_vault();
        let sealed = export_sealed(
            &source,
            "pw",
            ExportOptions {
                include_secrets: false,
            },
            |_, _| None,
        )
        .unwrap();

        let target = Vault::in_memory().unwrap();
        import_sealed(&target, "pw", &sealed, |_, _, _| Ok(())).unwrap();
        import_sealed(&target, "pw", &sealed, |_, _, _| Ok(())).unwrap();

        // Two independent copies of the tree; ids were reissued both times, so
        // nothing overwrote anything.
        let tree = target.tree().unwrap();
        assert_eq!(tree.folders.len(), 2);
        assert_eq!(tree.hosts.len(), 6);
    }

    #[test]
    fn secrets_travel_only_when_asked_and_land_back_in_the_keychain() {
        let source = seeded_vault();
        let web_id = source
            .tree()
            .unwrap()
            .hosts
            .iter()
            .find(|h| h.name == "web")
            .unwrap()
            .id
            .clone();

        // A fake keychain the export reads from.
        let mut store = HashMap::new();
        store.insert((web_id.clone(), "password"), "hunter2".to_string());

        let read = |id: &HostId, slot: SecretSlot| {
            let key = (id.clone(), slot_name(slot));
            store.get(&key).cloned()
        };
        let sealed = export_sealed(
            &source,
            "pw",
            ExportOptions {
                include_secrets: true,
            },
            read,
        )
        .unwrap();

        // Import into a fresh vault, capturing what would be written.
        let target = Vault::in_memory().unwrap();
        let mut written: Vec<(HostId, &'static str, String)> = Vec::new();
        let summary = import_sealed(&target, "pw", &sealed, |id, slot, secret| {
            written.push((id.clone(), slot_name(slot), secret.to_string()));
            Ok(())
        })
        .unwrap();

        assert_eq!(summary.secrets, 1);
        assert_eq!(written.len(), 1);
        let (new_id, slot, secret) = &written[0];
        assert_eq!(slot, &"password");
        assert_eq!(secret, "hunter2");

        // The secret was filed under the host's *new* id, and the cache flag
        // was set so the UI knows a password exists.
        let web = target
            .tree()
            .unwrap()
            .hosts
            .into_iter()
            .find(|h| h.name == "web")
            .unwrap();
        assert_eq!(new_id, &web.id);
        assert!(web.has_saved_password);
    }

    #[test]
    fn an_export_without_secrets_carries_none_even_if_the_keychain_has_them() {
        let source = seeded_vault();
        let sealed = export_sealed(
            &source,
            "pw",
            ExportOptions {
                include_secrets: false,
            },
            |_, _| Some("should-not-be-read".into()),
        )
        .unwrap();

        let target = Vault::in_memory().unwrap();
        let mut writes = 0;
        let summary = import_sealed(&target, "pw", &sealed, |_, _, _| {
            writes += 1;
            Ok(())
        })
        .unwrap();
        assert_eq!(summary.secrets, 0);
        assert_eq!(writes, 0);
    }

    #[test]
    fn the_wrong_passphrase_will_not_open_an_export() {
        let source = seeded_vault();
        let sealed = export_sealed(
            &source,
            "right",
            ExportOptions {
                include_secrets: false,
            },
            |_, _| None,
        )
        .unwrap();

        let target = Vault::in_memory().unwrap();
        let err = import_sealed(&target, "wrong", &sealed, |_, _, _| Ok(())).unwrap_err();
        assert_eq!(err.code(), "CRYPTO_ERROR");
        // Nothing was imported.
        assert!(target.tree().unwrap().hosts.is_empty());
    }

    #[test]
    fn an_empty_passphrase_is_refused_before_anything_is_sealed() {
        let source = seeded_vault();
        let err = export_sealed(
            &source,
            "",
            ExportOptions {
                include_secrets: false,
            },
            |_, _| None,
        )
        .unwrap_err();
        assert_eq!(err.code(), "CRYPTO_ERROR");
    }

    #[test]
    fn a_foreign_sealed_file_is_refused_with_a_clear_error() {
        // Correctly sealed, but the plaintext is not an export document.
        let sealed = crypto::seal("pw", b"{\"hello\":\"world\"}").unwrap();
        let target = Vault::in_memory().unwrap();
        let err = import_sealed(&target, "pw", &sealed, |_, _, _| Ok(())).unwrap_err();
        assert_eq!(err.code(), "CRYPTO_ERROR");
    }

    fn slot_name(slot: SecretSlot) -> &'static str {
        match slot {
            SecretSlot::Password => "password",
            SecretSlot::KeyPassphrase => "passphrase",
        }
    }
}
