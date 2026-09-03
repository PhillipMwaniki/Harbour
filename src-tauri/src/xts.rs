//! Reading an Xshell backup (`.xts`).
//!
//! A `.xts` is a plain ZIP of the whole NetSarang profile. This module knows
//! the layout and hands each part to the module that understands it - it does
//! no parsing of its own, only extraction and text decoding:
//!
//! | Inside the archive | What it is | Read by |
//! | --- | --- | --- |
//! | `Xshell/<folders>/<name>.xsh` | sessions, folder tree included | `vault::xshell` |
//! | `com/SECSH/HostKeys/key_<host>_<port>.pub` | host keys the user has trusted | `ssh::known_hosts` |
//! | `xsl/ColorScheme Files/*.scs` | colour schemes | `settings::scheme` |
//! | `xsl/HighlightSet Files/*.hls` | highlight sets | `settings::highlight` |
//!
//! The archive also holds `com/SECSH/UserKeys/*.pri` - the user's private
//! keys - and a credential file. **Nothing here reads them, and nothing may.**
//! They are not ours to copy, and a migration tool that quietly duplicated
//! someone's private keys would be a worse outcome than no migration.

use std::fs::File;
use std::io::{self, Read};
use std::path::{Path, PathBuf};

use zip::ZipArchive;

use crate::text;

/// A session file, with the folder path the archive filed it under.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct SessionFile {
    pub folder: Vec<String>,
    /// The file stem, which is the session's name.
    pub name: String,
    pub text: String,
}

/// A trusted host key, addressed the way the archive names it.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct HostKeyFile {
    pub host: String,
    pub port: u16,
    pub text: String,
}

/// A colour scheme or highlight set: one file, named by its stem.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct NamedFile {
    pub name: String,
    pub text: String,
}

/// What a walk found, plus anything it had to leave behind.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Found<T> {
    pub files: Vec<T>,
    /// Entries in the right place with the wrong shape, and why.
    pub notes: Vec<String>,
}

// Hand-written so that `Found<T>` is empty-constructible for any `T`; the
// derive would demand `T: Default`, which a key file has no sensible one of.
impl<T> Default for Found<T> {
    fn default() -> Self {
        Self {
            files: Vec::new(),
            notes: Vec::new(),
        }
    }
}

pub struct Archive {
    zip: ZipArchive<File>,
    path: PathBuf,
}

const SESSIONS: &str = "Xshell";
const HOST_KEYS: &str = "com/SECSH/HostKeys";
const COLOR_SCHEMES: &str = "xsl/ColorScheme Files";
const HIGHLIGHT_SETS: &str = "xsl/HighlightSet Files";

impl Archive {
    /// Whether `path` is a backup rather than an export directory: by
    /// extension, or by the ZIP signature for a renamed file.
    pub fn is_archive(path: &Path) -> bool {
        if !path.is_file() {
            return false;
        }
        if path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xts"))
        {
            return true;
        }
        let mut magic = [0u8; 4];
        File::open(path)
            .and_then(|mut file| file.read_exact(&mut magic))
            .map(|()| magic == *b"PK\x03\x04")
            .unwrap_or(false)
    }

    pub fn open(path: &Path) -> io::Result<Self> {
        let zip = ZipArchive::new(File::open(path)?).map_err(io::Error::other)?;
        Ok(Self {
            zip,
            path: path.to_path_buf(),
        })
    }

    pub fn path(&self) -> &Path {
        &self.path
    }

    /// Every `.xsh` under `Xshell/`, with the directories between as folders.
    pub fn sessions(&mut self) -> io::Result<Vec<SessionFile>> {
        let mut files = Vec::new();
        for (components, text) in self.read_all(SESSIONS, "xsh")? {
            let (name, folder) = split_name(components);
            files.push(SessionFile { folder, name, text });
        }
        // Stable ordering: folder first, then session name, so the review
        // list reads the way the session manager did.
        files.sort_by(|a, b| {
            a.folder
                .cmp(&b.folder)
                .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
        });
        Ok(files)
    }

    /// The host keys Xshell had accepted, named `key_<host>_<port>.pub`.
    pub fn host_keys(&mut self) -> io::Result<Found<HostKeyFile>> {
        let mut found = Found::default();
        for (components, text) in self.read_all(HOST_KEYS, "pub")? {
            let (stem, _) = split_name(components);
            match host_and_port(&stem) {
                Some((host, port)) => found.files.push(HostKeyFile { host, port, text }),
                None => found.notes.push(format!(
                    "{HOST_KEYS}/{stem}.pub: not named key_<host>_<port>, so there is no host to trust it for"
                )),
            }
        }
        found
            .files
            .sort_by(|a, b| a.host.cmp(&b.host).then(a.port.cmp(&b.port)));
        Ok(found)
    }

    pub fn color_schemes(&mut self) -> io::Result<Vec<NamedFile>> {
        self.named(COLOR_SCHEMES, "scs")
    }

    pub fn highlight_sets(&mut self) -> io::Result<Vec<NamedFile>> {
        self.named(HIGHLIGHT_SETS, "hls")
    }

    fn named(&mut self, prefix: &str, extension: &str) -> io::Result<Vec<NamedFile>> {
        let mut files: Vec<NamedFile> = self
            .read_all(prefix, extension)?
            .into_iter()
            .map(|(components, text)| {
                let (name, _) = split_name(components);
                NamedFile { name, text }
            })
            .collect();
        files.sort_by_key(|file| file.name.to_lowercase());
        Ok(files)
    }

    /// Decoded text of every file under `prefix` with `extension`, as the
    /// path components below the prefix. Directories, anything outside the
    /// prefix and anything whose name tries to escape it are skipped.
    fn read_all(
        &mut self,
        prefix: &str,
        extension: &str,
    ) -> io::Result<Vec<(Vec<String>, String)>> {
        let wanted: Vec<&str> = prefix.split('/').collect();
        let mut out = Vec::new();

        for index in 0..self.zip.len() {
            let mut entry = self.zip.by_index(index).map_err(io::Error::other)?;
            if entry.is_dir() {
                continue;
            }
            // `enclosed_name` refuses `..` and absolute names; an archive is
            // untrusted input like any other file.
            let Some(name) = entry.enclosed_name() else {
                continue;
            };
            let components: Vec<String> = name
                .components()
                .map(|part| part.as_os_str().to_string_lossy().into_owned())
                .collect();

            let inside = components.len() > wanted.len()
                && components
                    .iter()
                    .zip(&wanted)
                    .all(|(have, want)| have.eq_ignore_ascii_case(want));
            if !inside {
                continue;
            }
            let has_extension = Path::new(components.last().map(String::as_str).unwrap_or(""))
                .extension()
                .is_some_and(|ext| ext.eq_ignore_ascii_case(extension));
            if !has_extension {
                continue;
            }

            let mut bytes = Vec::with_capacity(entry.size() as usize);
            entry.read_to_end(&mut bytes)?;
            out.push((components[wanted.len()..].to_vec(), text::decode(&bytes)));
        }
        Ok(out)
    }
}

/// Splits `["Prod", "EU", "db-1.xsh"]` into `("db-1", ["Prod", "EU"])`.
fn split_name(mut components: Vec<String>) -> (String, Vec<String>) {
    let file = components.pop().unwrap_or_default();
    let stem = Path::new(&file)
        .file_stem()
        .map(|stem| stem.to_string_lossy().into_owned())
        .unwrap_or(file);
    (stem, components)
}

/// `key_fms.lwb.mw_38762` -> `("fms.lwb.mw", 38762)`. The port is after the
/// *last* underscore, so a host name containing one still reads.
fn host_and_port(stem: &str) -> Option<(String, u16)> {
    let rest = stem.strip_prefix("key_")?;
    let (host, port) = rest.rsplit_once('_')?;
    let port = port.parse::<u16>().ok().filter(|port| *port != 0)?;
    if host.is_empty() {
        return None;
    }
    Some((host.to_string(), port))
}

#[cfg(test)]
pub(crate) mod test_support {
    //! Builds `.xts`-shaped archives for tests, here and in the modules that
    //! read them.

    use std::io::Write;
    use std::path::PathBuf;

    use zip::write::SimpleFileOptions;
    use zip::CompressionMethod;

    /// UTF-16LE with a byte order mark: what Xshell writes.
    pub fn utf16(text: &str) -> Vec<u8> {
        let mut bytes = vec![0xff, 0xfe];
        for unit in text.encode_utf16() {
            bytes.extend_from_slice(&unit.to_le_bytes());
        }
        bytes
    }

    /// Writes an archive with the given entries. Alternates between stored and
    /// deflated, since a real backup uses both.
    pub fn archive(name: &str, entries: &[(&str, Vec<u8>)]) -> PathBuf {
        let dir = std::env::temp_dir().join(format!("harbour-xts-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join(name);
        let mut writer = zip::ZipWriter::new(std::fs::File::create(&path).unwrap());
        for (index, (entry, bytes)) in entries.iter().enumerate() {
            let method = if index % 2 == 0 {
                CompressionMethod::Stored
            } else {
                CompressionMethod::Deflated
            };
            writer
                .start_file(
                    *entry,
                    SimpleFileOptions::default().compression_method(method),
                )
                .unwrap();
            writer.write_all(bytes).unwrap();
        }
        writer.finish().unwrap();
        path
    }

    pub fn cleanup(path: &std::path::Path) {
        if let Some(dir) = path.parent() {
            std::fs::remove_dir_all(dir).ok();
        }
    }
}

#[cfg(test)]
mod tests {
    use super::test_support::{archive, cleanup, utf16};
    use super::*;

    const SESSION: &str = "[CONNECTION]\r\nProtocol=SSH\r\nHost=web.example.com\r\nPort=22\r\n";

    fn backup() -> PathBuf {
        archive(
            "backup.xts",
            &[
                ("Xshell/", Vec::new()),
                ("Xshell/jump.xsh", utf16(SESSION)),
                ("Xshell/Prod/EU/db-1.xsh", utf16(SESSION)),
                ("Xshell/Prod/notes.txt", b"not a session".to_vec()),
                ("Xftp/site.xfp", utf16("[CONNECTION]\r\nHost=ftp\r\n")),
                (
                    "com/SECSH/HostKeys/key_web.example.com_22.pub",
                    b"---- BEGIN SSH2 PUBLIC KEY ----\n".to_vec(),
                ),
                (
                    "com/SECSH/HostKeys/key_fms.lwb.mw_38762.pub",
                    b"---- BEGIN SSH2 PUBLIC KEY ----\n".to_vec(),
                ),
                (
                    "com/SECSH/HostKeys/random.pub",
                    b"---- BEGIN SSH2 PUBLIC KEY ----\n".to_vec(),
                ),
                ("com/SECSH/UserKeys/id_rsa.pri", b"PRIVATE".to_vec()),
                (
                    "xsl/ColorScheme Files/Obsidian.scs",
                    utf16("[Color Scheme]\r\ntext=cdcdcd\r\n"),
                ),
                (
                    "xsl/ColorScheme Files/ColorInfo.ini",
                    utf16("[Info]\r\nVersion=6.0\r\n"),
                ),
                (
                    "xsl/HighlightSet Files/Sample.hls",
                    utf16("[Keyword_0]\r\nKeyword=x\r\n"),
                ),
                ("xts.zcf", b"[SessionInfo]\r\nVersion=6.0\r\n".to_vec()),
            ],
        )
    }

    #[test]
    fn recognises_a_backup_by_extension_or_signature() {
        let path = backup();
        assert!(Archive::is_archive(&path));

        let renamed = path.with_extension("bin");
        std::fs::copy(&path, &renamed).unwrap();
        assert!(Archive::is_archive(&renamed));

        let dir = path.parent().unwrap();
        assert!(!Archive::is_archive(dir));
        let text = dir.join("plain.xts");
        std::fs::write(&text, "not a zip").unwrap();
        // The extension wins: a .xts that is not a ZIP is still handed to the
        // archive reader, which reports it properly.
        assert!(Archive::is_archive(&text));
        cleanup(&path);
    }

    #[test]
    fn lists_sessions_with_their_folders_and_decodes_the_text() {
        let path = backup();
        let sessions = Archive::open(&path).unwrap().sessions().unwrap();
        cleanup(&path);

        assert_eq!(sessions.len(), 2);
        assert_eq!(sessions[0].name, "jump");
        assert!(sessions[0].folder.is_empty());
        assert_eq!(sessions[1].name, "db-1");
        assert_eq!(sessions[1].folder, vec!["Prod", "EU"]);
        assert_eq!(sessions[1].text, SESSION);
    }

    #[test]
    fn host_keys_are_addressed_by_the_file_name() {
        let path = backup();
        let found = Archive::open(&path).unwrap().host_keys().unwrap();
        cleanup(&path);

        assert_eq!(found.files.len(), 2);
        assert_eq!(found.files[0].host, "fms.lwb.mw");
        assert_eq!(found.files[0].port, 38762);
        assert_eq!(found.files[1].host, "web.example.com");
        assert_eq!(found.files[1].port, 22);
        assert_eq!(found.notes.len(), 1);
        assert!(found.notes[0].contains("random.pub"));
    }

    #[test]
    fn schemes_and_highlight_sets_are_picked_out_by_extension() {
        let path = backup();
        let mut archive = Archive::open(&path).unwrap();
        let schemes = archive.color_schemes().unwrap();
        let sets = archive.highlight_sets().unwrap();
        cleanup(&path);

        assert_eq!(schemes.len(), 1, "ColorInfo.ini is not a scheme");
        assert_eq!(schemes[0].name, "Obsidian");
        assert_eq!(schemes[0].text, "[Color Scheme]\r\ntext=cdcdcd\r\n");
        assert_eq!(sets.len(), 1);
        assert_eq!(sets[0].name, "Sample");
    }

    /// The private keys in a backup are never read. There is no accessor for
    /// them, and no walk reaches their directory.
    #[test]
    fn nothing_reads_the_private_keys() {
        let path = backup();
        let mut archive = Archive::open(&path).unwrap();
        let everything: Vec<String> = archive
            .sessions()
            .unwrap()
            .into_iter()
            .map(|s| s.text)
            .chain(
                archive
                    .host_keys()
                    .unwrap()
                    .files
                    .into_iter()
                    .map(|k| k.text),
            )
            .chain(archive.color_schemes().unwrap().into_iter().map(|f| f.text))
            .chain(
                archive
                    .highlight_sets()
                    .unwrap()
                    .into_iter()
                    .map(|f| f.text),
            )
            .collect();
        cleanup(&path);

        assert!(everything.iter().all(|text| !text.contains("PRIVATE")));
    }

    #[test]
    fn a_file_that_is_not_a_zip_is_an_error_not_a_panic() {
        let dir = std::env::temp_dir().join(format!("harbour-xts-bad-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        let path = dir.join("bad.xts");
        std::fs::write(&path, "not a zip at all").unwrap();

        assert!(Archive::open(&path).is_err());
        std::fs::remove_dir_all(dir).ok();
    }

    #[test]
    fn host_key_names_split_on_the_last_underscore() {
        assert_eq!(
            host_and_port("key_my_host_2222"),
            Some(("my_host".to_string(), 2222))
        );
        assert_eq!(host_and_port("key_web_22"), Some(("web".to_string(), 22)));
        assert_eq!(host_and_port("key_web"), None);
        assert_eq!(host_and_port("key__22"), None);
        assert_eq!(host_and_port("key_web_0"), None);
        assert_eq!(host_and_port("other_web_22"), None);
    }
}
