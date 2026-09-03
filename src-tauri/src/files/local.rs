//! The local side of the file panes: `std::fs`, made to look like SFTP.

use std::path::{Path, PathBuf};
use std::time::UNIX_EPOCH;

use crate::error::{AppError, AppResult};
use crate::files::{Entry, EntryKind, Listing};

fn fail(path: &Path, err: impl std::fmt::Display) -> AppError {
    AppError::Files {
        path: path.display().to_string(),
        reason: err.to_string(),
    }
}

/// The user's home directory, which is where the local pane starts.
pub fn home() -> AppResult<String> {
    dirs::home_dir()
        .map(|home| home.display().to_string())
        .ok_or_else(|| AppError::Files {
            path: "~".into(),
            reason: "this machine has no home directory".into(),
        })
}

/// The top of the local tree: every drive that answers on Windows, `/`
/// elsewhere. The pane offers these when the user goes up past a root.
pub fn roots() -> Vec<String> {
    #[cfg(windows)]
    {
        (b'A'..=b'Z')
            .map(|letter| format!("{}:\\", letter as char))
            .filter(|drive| Path::new(drive).exists())
            .collect()
    }
    #[cfg(not(windows))]
    {
        vec!["/".to_string()]
    }
}

/// Lists a directory. The path comes back canonical, so `..` and symlinked
/// directories resolve to where the user actually is.
pub fn list(path: &Path) -> AppResult<Listing> {
    let canonical = dunce_canonicalize(path).map_err(|err| fail(path, err))?;
    let read = std::fs::read_dir(&canonical).map_err(|err| fail(&canonical, err))?;

    let mut entries = Vec::new();
    for item in read {
        let item = match item {
            Ok(item) => item,
            // One unreadable entry is not a reason to hide the directory.
            Err(err) => {
                tracing::debug!(dir = %canonical.display(), error = %err, "skipped an entry");
                continue;
            }
        };
        entries.push(entry(&item));
    }

    Ok(Listing {
        path: canonical.display().to_string(),
        parent: canonical
            .parent()
            .map(|parent| parent.display().to_string()),
        entries,
    })
}

pub fn mkdir(path: &Path) -> AppResult<()> {
    std::fs::create_dir(path).map_err(|err| fail(path, err))
}

pub fn rename(from: &Path, to: &Path) -> AppResult<()> {
    std::fs::rename(from, to).map_err(|err| fail(from, err))
}

/// Removes a file, an empty directory, or - with `recursive` - a directory and
/// everything under it. A symlink is removed as a link, never followed.
pub fn remove(path: &Path, recursive: bool) -> AppResult<()> {
    let meta = std::fs::symlink_metadata(path).map_err(|err| fail(path, err))?;
    let result = if meta.is_dir() {
        if recursive {
            std::fs::remove_dir_all(path)
        } else {
            std::fs::remove_dir(path)
        }
    } else {
        std::fs::remove_file(path)
    };
    result.map_err(|err| fail(path, err))
}

fn entry(item: &std::fs::DirEntry) -> Entry {
    let name = item.file_name().to_string_lossy().into_owned();
    let path = item.path();

    // `symlink_metadata` says what the entry *is*; `metadata` follows a link
    // to say what it points at. Both matter: the first for the icon, the
    // second for whether a double-click can enter it.
    let own = item
        .metadata()
        .or_else(|_| std::fs::symlink_metadata(&path));
    let symlink = own.as_ref().is_ok_and(|meta| meta.file_type().is_symlink());
    let target = if symlink {
        std::fs::metadata(&path).ok()
    } else {
        own.as_ref().ok().cloned()
    };

    let kind = match &target {
        Some(meta) if meta.is_dir() => EntryKind::Dir,
        Some(meta) if meta.is_file() => EntryKind::File,
        _ => EntryKind::Other,
    };

    Entry {
        hidden: is_hidden(&name, own.as_ref().ok()),
        name,
        kind,
        symlink,
        size: target
            .as_ref()
            .filter(|_| kind == EntryKind::File)
            .map(|meta| meta.len()),
        modified: target.as_ref().and_then(|meta| {
            meta.modified()
                .ok()
                .and_then(|time| time.duration_since(UNIX_EPOCH).ok())
                .map(|since| since.as_secs() as i64)
        }),
        permissions: target.as_ref().and_then(unix_mode),
        owner: None,
        group: None,
    }
}

fn is_hidden(name: &str, meta: Option<&std::fs::Metadata>) -> bool {
    if name.starts_with('.') {
        return true;
    }
    #[cfg(windows)]
    {
        use std::os::windows::fs::MetadataExt;
        const FILE_ATTRIBUTE_HIDDEN: u32 = 0x2;
        if let Some(meta) = meta {
            return meta.file_attributes() & FILE_ATTRIBUTE_HIDDEN != 0;
        }
    }
    let _ = meta;
    false
}

#[cfg(unix)]
fn unix_mode(meta: &std::fs::Metadata) -> Option<u32> {
    use std::os::unix::fs::PermissionsExt;
    Some(meta.permissions().mode() & 0o7777)
}

#[cfg(not(unix))]
fn unix_mode(_meta: &std::fs::Metadata) -> Option<u32> {
    None
}

/// `std::fs::canonicalize` on Windows yields `\\?\C:\...`, a form the user has
/// never seen and other programs choke on. Strip it back to a plain path.
fn dunce_canonicalize(path: &Path) -> std::io::Result<PathBuf> {
    let canonical = std::fs::canonicalize(path)?;
    let text = canonical.to_string_lossy();
    if let Some(stripped) = text.strip_prefix(r"\\?\") {
        // UNC paths (`\\?\UNC\server\share`) keep a different prefix.
        if let Some(unc) = stripped.strip_prefix(r"UNC\") {
            return Ok(PathBuf::from(format!(r"\\{unc}")));
        }
        return Ok(PathBuf::from(stripped));
    }
    Ok(canonical)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("harbour-local-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn lists_files_and_directories_with_their_kinds() {
        let dir = temp();
        std::fs::create_dir(dir.join("sub")).unwrap();
        std::fs::write(dir.join("notes.txt"), "hello").unwrap();
        std::fs::write(dir.join(".hidden"), "").unwrap();

        let listing = list(&dir).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert!(listing.parent.is_some());
        let by_name = |name: &str| listing.entries.iter().find(|e| e.name == name).unwrap();
        assert_eq!(by_name("sub").kind, EntryKind::Dir);
        assert_eq!(by_name("sub").size, None);
        assert_eq!(by_name("notes.txt").kind, EntryKind::File);
        assert_eq!(by_name("notes.txt").size, Some(5));
        assert!(by_name("notes.txt").modified.is_some());
        assert!(by_name(".hidden").hidden);
        assert!(!by_name("notes.txt").hidden);
    }

    #[test]
    fn the_listed_path_is_canonical_and_plain() {
        let dir = temp();
        std::fs::create_dir(dir.join("sub")).unwrap();

        let listing = list(&dir.join("sub").join("..")).unwrap();
        std::fs::remove_dir_all(&dir).ok();

        assert!(!listing.path.contains(".."));
        assert!(!listing.path.starts_with(r"\\?\"), "{}", listing.path);
    }

    #[test]
    fn a_missing_directory_is_a_files_error() {
        let err = list(Path::new("/definitely/not/here/harbour")).unwrap_err();
        assert_eq!(err.code(), "FILES_ERROR");
    }

    #[test]
    fn a_root_has_no_parent_and_roots_are_not_empty() {
        let roots = roots();
        assert!(!roots.is_empty());
        let listing = list(Path::new(&roots[0])).unwrap();
        assert_eq!(listing.parent, None, "{}", listing.path);
    }

    #[test]
    fn makes_renames_and_removes() {
        let dir = temp();
        mkdir(&dir.join("made")).unwrap();
        assert!(dir.join("made").is_dir());

        rename(&dir.join("made"), &dir.join("moved")).unwrap();
        assert!(!dir.join("made").exists());
        assert!(dir.join("moved").is_dir());

        std::fs::write(dir.join("moved").join("inner.txt"), "x").unwrap();
        // A non-empty directory needs `recursive`; without it the error names
        // the directory rather than deleting what is in it.
        let err = remove(&dir.join("moved"), false).unwrap_err();
        assert_eq!(err.code(), "FILES_ERROR");
        assert!(dir.join("moved").join("inner.txt").exists());

        remove(&dir.join("moved"), true).unwrap();
        assert!(!dir.join("moved").exists());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn home_is_a_directory() {
        let home = home().unwrap();
        assert!(Path::new(&home).is_dir());
    }
}
