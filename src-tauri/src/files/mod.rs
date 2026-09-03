//! Directory listings, wherever the directory is.
//!
//! The SFTP pane and the local pane show the same thing - a path, its parent,
//! and what is in it - so they share one shape. Path arithmetic happens here
//! too: the frontend never has to know whether `..` of a path means dropping
//! a `/` component or a `\` one, because the listing carries its own parent.

pub mod local;

use serde::{Deserialize, Serialize};

/// What an entry is, once a symlink has been followed. A link that points at
/// a directory is a `Dir` with `symlink: true`, so double-clicking it works;
/// a dangling one is `Other`.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum EntryKind {
    Dir,
    File,
    Other,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Entry {
    pub name: String,
    pub kind: EntryKind,
    pub symlink: bool,
    /// A dotfile, or a file Windows marks hidden. The pane hides these by
    /// default; the listing still carries them so the toggle costs no round
    /// trip.
    pub hidden: bool,
    pub size: Option<u64>,
    /// Seconds since the Unix epoch.
    pub modified: Option<i64>,
    /// Unix mode bits, where the file system has them.
    pub permissions: Option<u32>,
    pub owner: Option<String>,
    pub group: Option<String>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct Listing {
    /// The directory listed, made absolute and canonical.
    pub path: String,
    /// `None` at a root: `/`, or a drive on Windows.
    pub parent: Option<String>,
    pub entries: Vec<Entry>,
}

/// The parent of a POSIX path, for the remote side. `/` has none.
pub fn posix_parent(path: &str) -> Option<String> {
    let trimmed = path.trim_end_matches('/');
    if trimmed.is_empty() {
        return None;
    }
    match trimmed.rsplit_once('/') {
        Some(("", _)) => Some("/".to_string()),
        Some((parent, _)) => Some(parent.to_string()),
        None => None,
    }
}

/// Joins a name onto a POSIX directory.
pub fn posix_join(dir: &str, name: &str) -> String {
    if dir.ends_with('/') {
        format!("{dir}{name}")
    } else {
        format!("{dir}/{name}")
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn posix_parents() {
        assert_eq!(posix_parent("/home/deploy"), Some("/home".into()));
        assert_eq!(posix_parent("/home"), Some("/".into()));
        assert_eq!(posix_parent("/home/"), Some("/".into()));
        assert_eq!(posix_parent("/"), None);
        assert_eq!(posix_parent(""), None);
        assert_eq!(posix_parent("relative"), None);
    }

    #[test]
    fn posix_join_does_not_double_the_slash() {
        assert_eq!(posix_join("/", "etc"), "/etc");
        assert_eq!(posix_join("/etc", "ssh"), "/etc/ssh");
        assert_eq!(posix_join("/etc/", "ssh"), "/etc/ssh");
    }

    #[test]
    fn entries_serialise_camel_case_with_lowercase_kinds() {
        let entry = Entry {
            name: "x".into(),
            kind: EntryKind::Dir,
            symlink: true,
            hidden: false,
            size: None,
            modified: Some(0),
            permissions: Some(0o755),
            owner: None,
            group: None,
        };
        let json = serde_json::to_value(&entry).unwrap();
        assert_eq!(json["kind"], "dir");
        assert_eq!(json["symlink"], true);
        assert!(json.get("hidden").is_some());
    }
}
