//! The SQLite host store.
//!
//! Small, synchronous and behind one lock. The tree holds hundreds of hosts,
//! not millions, and every operation is a handful of rows; a connection pool
//! and an async driver would be machinery in place of a mutex. Callers that
//! care about the UI thread wrap these in `spawn_blocking`.
//!
//! Nothing here stores a secret. See [`crate::vault::secrets`].

use std::path::Path;

use parking_lot::Mutex;
use rusqlite::{params, Connection, OptionalExtension, Row};

use crate::error::{AppError, AppResult};
use crate::vault::model::{Folder, FolderId, Host, HostAuth, HostInput, VaultTree};

/// Bumped whenever the schema changes; `migrate` walks from whatever the file
/// is at up to this.
const SCHEMA_VERSION: i64 = 1;

pub struct Vault {
    connection: Mutex<Connection>,
}

impl Vault {
    /// Opens - and creates, if needed - the vault at `path`.
    pub fn open(path: &Path) -> AppResult<Self> {
        if let Some(parent) = path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let connection = Connection::open(path).map_err(vault_error)?;
        Self::from_connection(connection)
    }

    /// An empty in-memory vault. Used by tests, and as a last resort if the
    /// real file cannot be opened - a broken vault should not stop the app
    /// from opening a terminal.
    pub fn in_memory() -> AppResult<Self> {
        Self::from_connection(Connection::open_in_memory().map_err(vault_error)?)
    }

    fn from_connection(connection: Connection) -> AppResult<Self> {
        // Without this, `ON DELETE CASCADE` is silently inert: SQLite defaults
        // foreign keys to off, per connection.
        connection
            .execute_batch("PRAGMA foreign_keys = ON; PRAGMA journal_mode = WAL;")
            .map_err(vault_error)?;
        migrate(&connection)?;
        Ok(Self {
            connection: Mutex::new(connection),
        })
    }

    // -----------------------------------------------------------------------
    // Reading
    // -----------------------------------------------------------------------

    pub fn tree(&self) -> AppResult<VaultTree> {
        let connection = self.connection.lock();

        let folders = connection
            .prepare("SELECT id, parent_id, name, position FROM folders ORDER BY position, name")
            .and_then(|mut statement| {
                statement
                    .query_map([], |row| {
                        Ok(Folder {
                            id: row.get(0)?,
                            parent_id: row.get(1)?,
                            name: row.get(2)?,
                            position: row.get(3)?,
                        })
                    })?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(vault_error)?;

        let hosts = connection
            .prepare(&format!("{HOST_COLUMNS} ORDER BY position, name"))
            .and_then(|mut statement| {
                statement
                    .query_map([], host_from_row)?
                    .collect::<rusqlite::Result<Vec<_>>>()
            })
            .map_err(vault_error)?;

        Ok(VaultTree { folders, hosts })
    }

    pub fn host(&self, id: &str) -> AppResult<Host> {
        self.connection
            .lock()
            .query_row(
                &format!("{HOST_COLUMNS} WHERE id = ?1"),
                params![id],
                host_from_row,
            )
            .optional()
            .map_err(vault_error)?
            .ok_or_else(|| AppError::HostNotFound(id.to_string()))
    }

    // -----------------------------------------------------------------------
    // Folders
    // -----------------------------------------------------------------------

    pub fn create_folder(&self, parent_id: Option<&str>, name: &str) -> AppResult<Folder> {
        let name = clean_name(name, "folder")?;
        let connection = self.connection.lock();
        if let Some(parent) = parent_id {
            require_folder(&connection, parent)?;
        }

        let id = uuid::Uuid::new_v4().to_string();
        let position = next_position(&connection, "folders", parent_id)?;
        connection
            .execute(
                "INSERT INTO folders (id, parent_id, name, position) VALUES (?1, ?2, ?3, ?4)",
                params![id, parent_id, name, position],
            )
            .map_err(vault_error)?;

        Ok(Folder {
            id,
            parent_id: parent_id.map(str::to_string),
            name,
            position,
        })
    }

    pub fn rename_folder(&self, id: &str, name: &str) -> AppResult<()> {
        let name = clean_name(name, "folder")?;
        let connection = self.connection.lock();
        let changed = connection
            .execute(
                "UPDATE folders SET name = ?2 WHERE id = ?1",
                params![id, name],
            )
            .map_err(vault_error)?;
        if changed == 0 {
            return Err(AppError::FolderNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Moves a folder under `parent_id`, or to the top level.
    pub fn move_folder(&self, id: &str, parent_id: Option<&str>) -> AppResult<()> {
        let connection = self.connection.lock();
        require_folder(&connection, id)?;

        if let Some(parent) = parent_id {
            require_folder(&connection, parent)?;
            // Reparenting a folder under itself or one of its own descendants
            // would detach that whole subtree from the root: it would still be
            // in the table, reachable from nothing, and would never render.
            if parent == id || is_descendant(&connection, parent, id)? {
                return Err(AppError::Vault(
                    "a folder cannot be moved inside itself".into(),
                ));
            }
        }

        let position = next_position(&connection, "folders", parent_id)?;
        connection
            .execute(
                "UPDATE folders SET parent_id = ?2, position = ?3 WHERE id = ?1",
                params![id, parent_id, position],
            )
            .map_err(vault_error)?;
        Ok(())
    }

    /// Deletes a folder and everything under it. The caller is responsible for
    /// confirming that with the user; the hosts inside go too, by cascade.
    pub fn delete_folder(&self, id: &str) -> AppResult<()> {
        let connection = self.connection.lock();
        let changed = connection
            .execute("DELETE FROM folders WHERE id = ?1", params![id])
            .map_err(vault_error)?;
        if changed == 0 {
            return Err(AppError::FolderNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Walks `path`, creating any folder that is not there yet, and returns
    /// the innermost id. An empty path means the top level.
    ///
    /// This is what an import uses to mirror a directory tree.
    pub fn ensure_folder_path(&self, path: &[String]) -> AppResult<Option<FolderId>> {
        let mut parent: Option<FolderId> = None;
        for segment in path {
            let name = segment.trim();
            if name.is_empty() {
                continue;
            }
            let existing = {
                let connection = self.connection.lock();
                child_folder(&connection, parent.as_deref(), name)?
            };
            parent = Some(match existing {
                Some(id) => id,
                None => self.create_folder(parent.as_deref(), name)?.id,
            });
        }
        Ok(parent)
    }

    // -----------------------------------------------------------------------
    // Hosts
    // -----------------------------------------------------------------------

    pub fn create_host(&self, input: HostInput) -> AppResult<Host> {
        let input = validated(input)?;
        let connection = self.connection.lock();
        if let Some(folder) = input.folder_id.as_deref() {
            require_folder(&connection, folder)?;
        }

        let id = uuid::Uuid::new_v4().to_string();
        let position = next_position(&connection, "hosts", input.folder_id.as_deref())?;
        connection
            .execute(
                "INSERT INTO hosts (
                     id, folder_id, name, hostname, port, username, description,
                     use_agent, key_path, use_password, has_saved_password, position
                 ) VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10, 0, ?11)",
                params![
                    id,
                    input.folder_id,
                    input.name,
                    input.hostname,
                    input.port,
                    input.username,
                    input.description,
                    input.auth.use_agent,
                    input.auth.key_path,
                    input.auth.use_password,
                    position,
                ],
            )
            .map_err(vault_error)?;

        Ok(Host {
            id,
            folder_id: input.folder_id,
            name: input.name,
            hostname: input.hostname,
            port: input.port,
            username: input.username,
            description: input.description,
            auth: input.auth,
            has_saved_password: false,
            position,
        })
    }

    pub fn update_host(&self, id: &str, input: HostInput) -> AppResult<Host> {
        let input = validated(input)?;
        {
            let connection = self.connection.lock();
            if let Some(folder) = input.folder_id.as_deref() {
                require_folder(&connection, folder)?;
            }
            let changed = connection
                .execute(
                    "UPDATE hosts SET
                         folder_id = ?2, name = ?3, hostname = ?4, port = ?5,
                         username = ?6, description = ?7, use_agent = ?8,
                         key_path = ?9, use_password = ?10
                     WHERE id = ?1",
                    params![
                        id,
                        input.folder_id,
                        input.name,
                        input.hostname,
                        input.port,
                        input.username,
                        input.description,
                        input.auth.use_agent,
                        input.auth.key_path,
                        input.auth.use_password,
                    ],
                )
                .map_err(vault_error)?;
            if changed == 0 {
                return Err(AppError::HostNotFound(id.to_string()));
            }
        }
        self.host(id)
    }

    pub fn delete_host(&self, id: &str) -> AppResult<()> {
        let changed = self
            .connection
            .lock()
            .execute("DELETE FROM hosts WHERE id = ?1", params![id])
            .map_err(vault_error)?;
        if changed == 0 {
            return Err(AppError::HostNotFound(id.to_string()));
        }
        Ok(())
    }

    pub fn move_host(&self, id: &str, folder_id: Option<&str>) -> AppResult<()> {
        let connection = self.connection.lock();
        if let Some(folder) = folder_id {
            require_folder(&connection, folder)?;
        }
        let position = next_position(&connection, "hosts", folder_id)?;
        let changed = connection
            .execute(
                "UPDATE hosts SET folder_id = ?2, position = ?3 WHERE id = ?1",
                params![id, folder_id, position],
            )
            .map_err(vault_error)?;
        if changed == 0 {
            return Err(AppError::HostNotFound(id.to_string()));
        }
        Ok(())
    }

    /// Records whether the keychain is expected to hold a password for this
    /// host. The keychain stays authoritative; this only spares the UI a read.
    pub fn set_saved_password(&self, id: &str, saved: bool) -> AppResult<()> {
        let changed = self
            .connection
            .lock()
            .execute(
                "UPDATE hosts SET has_saved_password = ?2 WHERE id = ?1",
                params![id, saved],
            )
            .map_err(vault_error)?;
        if changed == 0 {
            return Err(AppError::HostNotFound(id.to_string()));
        }
        Ok(())
    }
}

// ---------------------------------------------------------------------------
// Schema
// ---------------------------------------------------------------------------

fn migrate(connection: &Connection) -> AppResult<()> {
    let version: i64 = connection
        .query_row("PRAGMA user_version", [], |row| row.get(0))
        .map_err(vault_error)?;

    if version >= SCHEMA_VERSION {
        return Ok(());
    }

    if version < 1 {
        connection
            .execute_batch(
                "CREATE TABLE folders (
                     id        TEXT PRIMARY KEY,
                     parent_id TEXT REFERENCES folders(id) ON DELETE CASCADE,
                     name      TEXT NOT NULL,
                     position  INTEGER NOT NULL
                 );
                 CREATE INDEX folders_parent ON folders(parent_id);

                 CREATE TABLE hosts (
                     id                 TEXT PRIMARY KEY,
                     folder_id          TEXT REFERENCES folders(id) ON DELETE CASCADE,
                     name               TEXT NOT NULL,
                     hostname           TEXT NOT NULL,
                     port               INTEGER NOT NULL,
                     username           TEXT NOT NULL,
                     description        TEXT,
                     use_agent          INTEGER NOT NULL DEFAULT 1,
                     key_path           TEXT,
                     use_password       INTEGER NOT NULL DEFAULT 1,
                     -- A cache of what the OS keychain holds. Never a secret.
                     has_saved_password INTEGER NOT NULL DEFAULT 0,
                     position           INTEGER NOT NULL
                 );
                 CREATE INDEX hosts_folder ON hosts(folder_id);",
            )
            .map_err(vault_error)?;
    }

    connection
        .pragma_update(None, "user_version", SCHEMA_VERSION)
        .map_err(vault_error)?;
    Ok(())
}

/// Every host read goes through the same column list, so a schema change
/// cannot leave one query behind.
const HOST_COLUMNS: &str = "SELECT id, folder_id, name, hostname, port, username, description, \
                            use_agent, key_path, use_password, has_saved_password, position \
                            FROM hosts";

fn host_from_row(row: &Row<'_>) -> rusqlite::Result<Host> {
    Ok(Host {
        id: row.get(0)?,
        folder_id: row.get(1)?,
        name: row.get(2)?,
        hostname: row.get(3)?,
        port: row.get(4)?,
        username: row.get(5)?,
        description: row.get(6)?,
        auth: HostAuth {
            use_agent: row.get(7)?,
            key_path: row.get(8)?,
            use_password: row.get(9)?,
        },
        has_saved_password: row.get(10)?,
        position: row.get(11)?,
    })
}

// ---------------------------------------------------------------------------
// Helpers
// ---------------------------------------------------------------------------

fn vault_error(err: rusqlite::Error) -> AppError {
    AppError::Vault(err.to_string())
}

fn clean_name(name: &str, what: &str) -> AppResult<String> {
    let name = name.trim();
    if name.is_empty() {
        return Err(AppError::Vault(format!("a {what} needs a name")));
    }
    Ok(name.to_string())
}

fn validated(input: HostInput) -> AppResult<HostInput> {
    let input = input.normalised();
    if input.hostname.is_empty() {
        return Err(AppError::Vault("a host needs a hostname".into()));
    }
    if input.username.is_empty() {
        return Err(AppError::Vault("a host needs a username".into()));
    }
    Ok(input)
}

fn require_folder(connection: &Connection, id: &str) -> AppResult<()> {
    let exists: Option<i64> = connection
        .query_row("SELECT 1 FROM folders WHERE id = ?1", params![id], |row| {
            row.get(0)
        })
        .optional()
        .map_err(vault_error)?;
    exists
        .map(|_| ())
        .ok_or_else(|| AppError::FolderNotFound(id.to_string()))
}

/// Is `candidate` somewhere below `ancestor`?
fn is_descendant(connection: &Connection, candidate: &str, ancestor: &str) -> AppResult<bool> {
    let mut current = Some(candidate.to_string());
    // The tree is acyclic by construction, but a bounded walk means a
    // corrupted file cannot hang the app.
    for _ in 0..1024 {
        let Some(id) = current else { return Ok(false) };
        if id == ancestor {
            return Ok(true);
        }
        current = connection
            .query_row(
                "SELECT parent_id FROM folders WHERE id = ?1",
                params![id],
                |row| row.get::<_, Option<String>>(0),
            )
            .optional()
            .map_err(vault_error)?
            .flatten();
    }
    Err(AppError::Vault("the folder tree contains a cycle".into()))
}

fn child_folder(
    connection: &Connection,
    parent_id: Option<&str>,
    name: &str,
) -> AppResult<Option<FolderId>> {
    // `parent_id IS ?1` rather than `=`, so the top level (NULL) matches.
    connection
        .query_row(
            "SELECT id FROM folders WHERE parent_id IS ?1 AND name = ?2",
            params![parent_id, name],
            |row| row.get(0),
        )
        .optional()
        .map_err(vault_error)
}

/// The next free slot among a parent's children, so new entries land at the
/// end rather than jumping to the front of an ordered list.
fn next_position(connection: &Connection, table: &str, parent: Option<&str>) -> AppResult<i64> {
    let column = if table == "folders" {
        "parent_id"
    } else {
        "folder_id"
    };
    let query = format!("SELECT COALESCE(MAX(position), -1) + 1 FROM {table} WHERE {column} IS ?1");
    connection
        .query_row(&query, params![parent], |row| row.get(0))
        .map_err(vault_error)
}

/// A host id doubles as its keychain key, so it must not be usable to address
/// some other entry. Ids are UUIDs, so this only guards against a caller
/// inventing one.
pub fn is_valid_id(id: &str) -> bool {
    !id.is_empty()
        && id.len() <= 64
        && id
            .chars()
            .all(|c| c.is_ascii_alphanumeric() || c == '-' || c == '_')
}

#[cfg(test)]
mod tests {
    use super::*;

    fn vault() -> Vault {
        Vault::in_memory().expect("an in-memory vault should open")
    }

    fn host_input(name: &str, folder: Option<&str>) -> HostInput {
        HostInput {
            folder_id: folder.map(str::to_string),
            name: name.into(),
            hostname: format!("{name}.example.com"),
            port: 22,
            username: "deploy".into(),
            description: None,
            auth: HostAuth::default(),
        }
    }

    #[test]
    fn a_new_vault_is_empty() {
        let tree = vault().tree().unwrap();
        assert!(tree.folders.is_empty());
        assert!(tree.hosts.is_empty());
    }

    #[test]
    fn opening_an_existing_vault_keeps_what_was_in_it() {
        let dir = std::env::temp_dir().join(format!("harbour-vault-{}", uuid::Uuid::new_v4()));
        let path = dir.join("vault.sqlite3");

        let id = {
            let vault = Vault::open(&path).unwrap();
            vault.create_host(host_input("web", None)).unwrap().id
        };

        let reopened = Vault::open(&path).unwrap();
        assert_eq!(reopened.host(&id).unwrap().name, "web");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn hosts_round_trip_through_the_store() {
        let vault = vault();
        let created = vault
            .create_host(HostInput {
                folder_id: None,
                name: "web".into(),
                hostname: "web.example.com".into(),
                port: 2222,
                username: "deploy".into(),
                description: Some("front end".into()),
                auth: HostAuth {
                    use_agent: false,
                    key_path: Some("~/.ssh/id_ed25519".into()),
                    use_password: true,
                },
            })
            .unwrap();

        let read = vault.host(&created.id).unwrap();
        assert_eq!(read, created);
        assert_eq!(read.port, 2222);
        assert_eq!(read.auth.key_path.as_deref(), Some("~/.ssh/id_ed25519"));
        assert!(!read.auth.use_agent);
        assert!(!read.has_saved_password);
    }

    #[test]
    fn a_host_without_a_hostname_is_refused() {
        let vault = vault();
        let mut input = host_input("web", None);
        input.hostname = "  ".into();

        let err = vault.create_host(input).unwrap_err();
        assert_eq!(err.code(), "VAULT_ERROR");
    }

    #[test]
    fn updating_a_host_keeps_its_id_and_replaces_its_fields() {
        let vault = vault();
        let created = vault.create_host(host_input("web", None)).unwrap();

        let mut input = host_input("web", None);
        input.name = "web (staging)".into();
        input.hostname = "staging.example.com".into();
        input.auth.use_password = false;
        let updated = vault.update_host(&created.id, input).unwrap();

        assert_eq!(updated.id, created.id);
        assert_eq!(updated.name, "web (staging)");
        assert_eq!(updated.hostname, "staging.example.com");
        assert!(!updated.auth.use_password);
    }

    #[test]
    fn unknown_hosts_and_folders_report_stable_codes() {
        let vault = vault();
        assert_eq!(vault.host("nope").unwrap_err().code(), "HOST_NOT_FOUND");
        assert_eq!(
            vault.delete_host("nope").unwrap_err().code(),
            "HOST_NOT_FOUND"
        );
        assert_eq!(
            vault.delete_folder("nope").unwrap_err().code(),
            "FOLDER_NOT_FOUND"
        );
        assert_eq!(
            vault.rename_folder("nope", "x").unwrap_err().code(),
            "FOLDER_NOT_FOUND"
        );
    }

    #[test]
    fn a_host_cannot_be_filed_under_a_folder_that_does_not_exist() {
        let vault = vault();
        let err = vault
            .create_host(host_input("web", Some("ghost")))
            .unwrap_err();
        assert_eq!(err.code(), "FOLDER_NOT_FOUND");
    }

    /// Deleting a folder is deliberately destructive; what must not happen is
    /// hosts surviving as orphans that no tree can reach.
    #[test]
    fn deleting_a_folder_takes_its_hosts_and_subfolders_with_it() {
        let vault = vault();
        let parent = vault.create_folder(None, "Production").unwrap();
        let child = vault.create_folder(Some(&parent.id), "Web").unwrap();
        vault
            .create_host(host_input("web", Some(&child.id)))
            .unwrap();
        vault
            .create_host(host_input("db", Some(&parent.id)))
            .unwrap();
        vault.create_host(host_input("laptop", None)).unwrap();

        vault.delete_folder(&parent.id).unwrap();

        let tree = vault.tree().unwrap();
        assert!(tree.folders.is_empty());
        assert_eq!(tree.hosts.len(), 1, "only the top-level host should remain");
        assert_eq!(tree.hosts[0].name, "laptop");
    }

    #[test]
    fn folders_can_be_moved_between_parents() {
        let vault = vault();
        let a = vault.create_folder(None, "A").unwrap();
        let b = vault.create_folder(None, "B").unwrap();

        vault.move_folder(&b.id, Some(&a.id)).unwrap();
        let tree = vault.tree().unwrap();
        let moved = tree.folders.iter().find(|f| f.id == b.id).unwrap();
        assert_eq!(moved.parent_id.as_deref(), Some(a.id.as_str()));

        vault.move_folder(&b.id, None).unwrap();
        let tree = vault.tree().unwrap();
        let moved = tree.folders.iter().find(|f| f.id == b.id).unwrap();
        assert_eq!(moved.parent_id, None);
    }

    /// The subtree would still be in the table but reachable from nothing, so
    /// it would vanish from the UI with no way to get it back.
    #[test]
    fn a_folder_cannot_be_moved_inside_its_own_subtree() {
        let vault = vault();
        let parent = vault.create_folder(None, "Parent").unwrap();
        let child = vault.create_folder(Some(&parent.id), "Child").unwrap();
        let grandchild = vault.create_folder(Some(&child.id), "Grandchild").unwrap();

        assert_eq!(
            vault
                .move_folder(&parent.id, Some(&parent.id))
                .unwrap_err()
                .code(),
            "VAULT_ERROR"
        );
        assert_eq!(
            vault
                .move_folder(&parent.id, Some(&grandchild.id))
                .unwrap_err()
                .code(),
            "VAULT_ERROR"
        );
        // The other direction is fine.
        assert!(vault.move_folder(&grandchild.id, Some(&parent.id)).is_ok());
    }

    #[test]
    fn hosts_can_be_moved_between_folders() {
        let vault = vault();
        let folder = vault.create_folder(None, "Production").unwrap();
        let host = vault.create_host(host_input("web", None)).unwrap();

        vault.move_host(&host.id, Some(&folder.id)).unwrap();
        assert_eq!(
            vault.host(&host.id).unwrap().folder_id.as_deref(),
            Some(folder.id.as_str())
        );

        vault.move_host(&host.id, None).unwrap();
        assert_eq!(vault.host(&host.id).unwrap().folder_id, None);
    }

    #[test]
    fn new_entries_go_to_the_end_of_their_parent() {
        let vault = vault();
        let first = vault.create_host(host_input("a", None)).unwrap();
        let second = vault.create_host(host_input("b", None)).unwrap();
        let third = vault.create_host(host_input("c", None)).unwrap();

        assert_eq!(first.position, 0);
        assert_eq!(second.position, 1);
        assert_eq!(third.position, 2);
    }

    /// Positions are per parent, so a new host in an empty folder starts at
    /// zero rather than continuing the top level's numbering.
    #[test]
    fn positions_are_counted_within_a_folder() {
        let vault = vault();
        vault.create_host(host_input("a", None)).unwrap();
        vault.create_host(host_input("b", None)).unwrap();
        let folder = vault.create_folder(None, "Production").unwrap();

        let inside = vault
            .create_host(host_input("c", Some(&folder.id)))
            .unwrap();
        assert_eq!(inside.position, 0);
    }

    #[test]
    fn a_folder_path_is_created_once_and_then_reused() {
        let vault = vault();
        let first = vault
            .ensure_folder_path(&["Customers".into(), "Acme".into()])
            .unwrap();
        let second = vault
            .ensure_folder_path(&["Customers".into(), "Acme".into()])
            .unwrap();

        assert_eq!(first, second);
        assert_eq!(vault.tree().unwrap().folders.len(), 2);
    }

    #[test]
    fn an_empty_folder_path_means_the_top_level() {
        let vault = vault();
        assert_eq!(vault.ensure_folder_path(&[]).unwrap(), None);
        assert_eq!(vault.ensure_folder_path(&["  ".into()]).unwrap(), None);
    }

    /// Two folders of the same name under different parents are different
    /// folders; the import path depends on this.
    #[test]
    fn the_same_name_under_different_parents_is_two_folders() {
        let vault = vault();
        let a = vault
            .ensure_folder_path(&["Customers".into(), "Web".into()])
            .unwrap();
        let b = vault
            .ensure_folder_path(&["Internal".into(), "Web".into()])
            .unwrap();

        assert_ne!(a, b);
        assert_eq!(vault.tree().unwrap().folders.len(), 4);
    }

    #[test]
    fn the_saved_password_flag_can_be_set_and_cleared() {
        let vault = vault();
        let host = vault.create_host(host_input("web", None)).unwrap();

        vault.set_saved_password(&host.id, true).unwrap();
        assert!(vault.host(&host.id).unwrap().has_saved_password);

        vault.set_saved_password(&host.id, false).unwrap();
        assert!(!vault.host(&host.id).unwrap().has_saved_password);
    }

    #[test]
    fn reopening_a_migrated_vault_does_not_migrate_it_again() {
        let dir = std::env::temp_dir().join(format!("harbour-vault-{}", uuid::Uuid::new_v4()));
        let path = dir.join("vault.sqlite3");

        Vault::open(&path).unwrap();
        // A second `CREATE TABLE` would fail, so this opening at all is the
        // assertion that the version check works.
        assert!(Vault::open(&path).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn ids_that_could_address_another_keychain_entry_are_rejected() {
        assert!(is_valid_id(&uuid::Uuid::new_v4().to_string()));
        assert!(!is_valid_id(""));
        assert!(!is_valid_id("../other"));
        assert!(!is_valid_id("has space"));
        assert!(!is_valid_id(&"x".repeat(65)));
    }
}
