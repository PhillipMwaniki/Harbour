//! Import of Xshell session files (`.xsh`).
//!
//! An Xshell export is a directory of `.xsh` files - INI-shaped text, one file
//! per session, with the session manager's folder tree mirrored as
//! subdirectories. This module turns that into [`ImportedHost`] records.
//!
//! **Passwords are deliberately not decoded.** Xshell stores `Password=` as a
//! ciphertext tied to the Windows account (and, when set, a master password),
//! with a scheme that differs between Xshell 5, 6 and 7. Reimplementing that
//! would be version-fragile and would mean writing recovered plaintext into a
//! new store. Instead the import records *that* a password existed
//! ([`ImportedHost::has_stored_password`]) so the UI can prompt for it once, on
//! first connect.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use serde::{Deserialize, Serialize};

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Protocol {
    Ssh,
    Telnet,
    Rlogin,
    Serial,
    Sftp,
    Ftp,
    Unknown,
}

impl Protocol {
    fn parse(raw: &str) -> Self {
        match raw.trim().to_ascii_lowercase().as_str() {
            "ssh" => Protocol::Ssh,
            "telnet" => Protocol::Telnet,
            "rlogin" => Protocol::Rlogin,
            "serial" => Protocol::Serial,
            "sftp" => Protocol::Sftp,
            "ftp" => Protocol::Ftp,
            _ => Protocol::Unknown,
        }
    }

    fn default_port(self) -> u16 {
        match self {
            Protocol::Ssh | Protocol::Sftp => 22,
            Protocol::Telnet => 23,
            Protocol::Rlogin => 513,
            Protocol::Ftp => 21,
            Protocol::Serial | Protocol::Unknown => 0,
        }
    }
}

#[derive(Debug, Clone, Copy, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "kebab-case")]
pub enum AuthMethod {
    Password,
    PublicKey,
    KeyboardInteractive,
    Gssapi,
    Other,
}

impl AuthMethod {
    fn parse(raw: &str) -> Self {
        let normalised = raw.trim().to_ascii_lowercase().replace([' ', '_'], "-");
        match normalised.as_str() {
            "password" => AuthMethod::Password,
            "public-key" | "publickey" => AuthMethod::PublicKey,
            "keyboard-interactive" => AuthMethod::KeyboardInteractive,
            "gssapi" => AuthMethod::Gssapi,
            _ => AuthMethod::Other,
        }
    }
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportedHost {
    /// Session name, taken from the file stem.
    pub name: String,
    /// Folder path, from the directories between the export root and the file.
    pub folder: Vec<String>,
    pub protocol: Protocol,
    pub hostname: String,
    pub port: u16,
    pub username: Option<String>,
    pub description: Option<String>,
    pub auth_methods: Vec<AuthMethod>,
    /// Name of the Xshell user key, which lives in Xshell's own key store; the
    /// user has to point us at the actual key file.
    pub key_name: Option<String>,
    pub encoding: Option<String>,
    /// A password was stored in the `.xsh` but was not decoded. See the module
    /// docs for why.
    pub has_stored_password: bool,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct ImportReport {
    pub hosts: Vec<ImportedHost>,
    /// Files that could not be read as sessions, with the reason.
    pub skipped: Vec<SkippedFile>,
}

#[derive(Debug, Clone, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SkippedFile {
    pub path: String,
    pub reason: String,
}

#[derive(Debug, Clone, PartialEq, Eq, thiserror::Error)]
pub enum ParseError {
    #[error("no [CONNECTION] section")]
    MissingConnection,
    #[error("no Host= entry")]
    MissingHost,
}

/// Case-insensitive INI, tolerant of the quirks real `.xsh` files show:
/// a UTF-8 BOM, CRLF endings, `;`/`#` comments, blank lines, and values that
/// themselves contain `=`.
fn parse_ini(contents: &str) -> BTreeMap<String, BTreeMap<String, String>> {
    let mut sections: BTreeMap<String, BTreeMap<String, String>> = BTreeMap::new();
    let mut current = String::new();

    for raw_line in contents.trim_start_matches('\u{feff}').lines() {
        let line = raw_line.trim();
        if line.is_empty() || line.starts_with(';') || line.starts_with('#') {
            continue;
        }
        if let Some(name) = line.strip_prefix('[').and_then(|l| l.strip_suffix(']')) {
            current = name.trim().to_ascii_uppercase();
            sections.entry(current.clone()).or_default();
            continue;
        }
        if let Some((key, value)) = line.split_once('=') {
            sections
                .entry(current.clone())
                .or_default()
                .insert(key.trim().to_ascii_uppercase(), value.trim().to_string());
        }
    }
    sections
}

fn get<'a>(
    sections: &'a BTreeMap<String, BTreeMap<String, String>>,
    section: &str,
    key: &str,
) -> Option<&'a str> {
    sections
        .get(section)?
        .get(key)
        .map(|value| value.as_str())
        .filter(|value| !value.is_empty())
}

/// Parses one `.xsh` file. `name` is the session name (the file stem) and
/// `folder` its path within the export.
pub fn parse_session(
    name: &str,
    folder: Vec<String>,
    contents: &str,
) -> Result<ImportedHost, ParseError> {
    let sections = parse_ini(contents);
    if !sections.contains_key("CONNECTION") {
        return Err(ParseError::MissingConnection);
    }

    let protocol = get(&sections, "CONNECTION", "PROTOCOL")
        .map(Protocol::parse)
        .unwrap_or(Protocol::Unknown);

    let hostname = get(&sections, "CONNECTION", "HOST")
        .ok_or(ParseError::MissingHost)?
        .to_string();

    let port = get(&sections, "CONNECTION", "PORT")
        .and_then(|value| value.parse::<u16>().ok())
        .filter(|port| *port != 0)
        .unwrap_or_else(|| protocol.default_port());

    // Xshell 5 and 6 differ on where authentication lives.
    let auth_section = if sections.contains_key("CONNECTION:AUTHENTICATION") {
        "CONNECTION:AUTHENTICATION"
    } else {
        "CONNECTION"
    };

    let auth_methods = get(&sections, auth_section, "METHOD")
        .map(|raw| raw.split(',').map(AuthMethod::parse).collect())
        .unwrap_or_default();

    Ok(ImportedHost {
        name: name.to_string(),
        folder,
        protocol,
        hostname,
        port,
        username: get(&sections, auth_section, "USERNAME").map(str::to_string),
        description: get(&sections, "CONNECTION", "DESCRIPTION").map(str::to_string),
        auth_methods,
        key_name: get(&sections, auth_section, "USERKEYNAME")
            .or_else(|| get(&sections, auth_section, "USERKEY"))
            .map(str::to_string),
        encoding: get(&sections, "TERMINAL", "ENCODING").map(str::to_string),
        has_stored_password: get(&sections, auth_section, "PASSWORD").is_some(),
    })
}

/// Walks an Xshell export directory, importing every `.xsh` it contains and
/// mirroring the directory tree as folder paths.
pub fn import_tree(root: &Path) -> std::io::Result<ImportReport> {
    let mut report = ImportReport {
        hosts: Vec::new(),
        skipped: Vec::new(),
    };
    visit(root, &mut Vec::new(), &mut report)?;
    // Stable ordering: folder first, then session name.
    report.hosts.sort_by(|a, b| {
        a.folder
            .cmp(&b.folder)
            .then_with(|| a.name.to_lowercase().cmp(&b.name.to_lowercase()))
    });
    Ok(report)
}

fn visit(dir: &Path, folder: &mut Vec<String>, report: &mut ImportReport) -> std::io::Result<()> {
    let mut entries: Vec<PathBuf> = std::fs::read_dir(dir)?
        .filter_map(Result::ok)
        .map(|entry| entry.path())
        .collect();
    entries.sort();

    for path in entries {
        if path.is_dir() {
            let name = path
                .file_name()
                .map(|n| n.to_string_lossy().into_owned())
                .unwrap_or_default();
            folder.push(name);
            visit(&path, folder, report)?;
            folder.pop();
            continue;
        }

        if !path
            .extension()
            .is_some_and(|ext| ext.eq_ignore_ascii_case("xsh"))
        {
            continue;
        }

        let name = path
            .file_stem()
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_default();

        match std::fs::read(&path) {
            // `.xsh` files are usually UTF-8 but may carry a BOM or stray
            // non-UTF-8 bytes in a description; lossy is the right trade here.
            Ok(bytes) => {
                let contents = String::from_utf8_lossy(&bytes);
                match parse_session(&name, folder.clone(), &contents) {
                    Ok(host) => report.hosts.push(host),
                    Err(err) => report.skipped.push(SkippedFile {
                        path: path.to_string_lossy().into_owned(),
                        reason: err.to_string(),
                    }),
                }
            }
            Err(err) => report.skipped.push(SkippedFile {
                path: path.to_string_lossy().into_owned(),
                reason: err.to_string(),
            }),
        }
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const XSHELL_6: &str = "\
[SessionInfo]
Version=6.0

[CONNECTION]
Protocol=SSH
Host=10.20.30.40
Port=2222
Description=web frontend, staging

[CONNECTION:AUTHENTICATION]
Method=Password,Public Key
UserName=deploy
Password=hK3Ns9LqQ1Z2vA==
UserKeyName=deploy_ed25519

[TERMINAL]
Type=xterm
Encoding=UTF-8
";

    #[test]
    fn parses_a_typical_xshell_6_session() {
        let host = parse_session("staging-web", vec!["Staging".into()], XSHELL_6).unwrap();

        assert_eq!(host.name, "staging-web");
        assert_eq!(host.folder, vec!["Staging"]);
        assert_eq!(host.protocol, Protocol::Ssh);
        assert_eq!(host.hostname, "10.20.30.40");
        assert_eq!(host.port, 2222);
        assert_eq!(host.username.as_deref(), Some("deploy"));
        assert_eq!(host.description.as_deref(), Some("web frontend, staging"));
        assert_eq!(host.key_name.as_deref(), Some("deploy_ed25519"));
        assert_eq!(host.encoding.as_deref(), Some("UTF-8"));
        assert_eq!(
            host.auth_methods,
            vec![AuthMethod::Password, AuthMethod::PublicKey]
        );
    }

    #[test]
    fn records_that_a_password_existed_without_decoding_it() {
        let host = parse_session("s", vec![], XSHELL_6).unwrap();
        assert!(host.has_stored_password);

        // The ciphertext must not survive anywhere in the imported record.
        let json = serde_json::to_string(&host).unwrap();
        assert!(
            !json.contains("hK3Ns9LqQ1Z2vA"),
            "password leaked into {json}"
        );
    }

    #[test]
    fn falls_back_to_the_protocol_default_port() {
        let contents = "[CONNECTION]\nProtocol=SSH\nHost=example.com\n";
        assert_eq!(parse_session("s", vec![], contents).unwrap().port, 22);

        let telnet = "[CONNECTION]\nProtocol=TELNET\nHost=example.com\nPort=0\n";
        assert_eq!(parse_session("s", vec![], telnet).unwrap().port, 23);
    }

    #[test]
    fn reads_xshell_5_style_files_with_auth_inside_connection() {
        let contents = "\
[CONNECTION]
Protocol=SSH
Host=legacy.internal
UserName=root
Method=Password
";
        let host = parse_session("legacy", vec![], contents).unwrap();
        assert_eq!(host.username.as_deref(), Some("root"));
        assert_eq!(host.auth_methods, vec![AuthMethod::Password]);
    }

    #[test]
    fn tolerates_bom_crlf_comments_and_odd_casing() {
        let contents = "\u{feff}; exported by Xshell\r\n\r\n[connection]\r\nprotocol=ssh\r\nHOST=box\r\nPort=22\r\n";
        let host = parse_session("box", vec![], contents).unwrap();
        assert_eq!(host.hostname, "box");
        assert_eq!(host.protocol, Protocol::Ssh);
    }

    #[test]
    fn keeps_values_that_contain_an_equals_sign() {
        let contents = "[CONNECTION]\nProtocol=SSH\nHost=box\nDescription=a=b=c\n";
        let host = parse_session("box", vec![], contents).unwrap();
        assert_eq!(host.description.as_deref(), Some("a=b=c"));
    }

    #[test]
    fn treats_empty_values_as_absent() {
        let contents = "[CONNECTION]\nProtocol=SSH\nHost=box\nDescription=\n\
[CONNECTION:AUTHENTICATION]\nUserName=\nPassword=\n";
        let host = parse_session("box", vec![], contents).unwrap();
        assert_eq!(host.description, None);
        assert_eq!(host.username, None);
        assert!(!host.has_stored_password);
    }

    #[test]
    fn unknown_protocols_do_not_lose_the_host() {
        let contents = "[CONNECTION]\nProtocol=QUIC-THING\nHost=box\n";
        let host = parse_session("box", vec![], contents).unwrap();
        assert_eq!(host.protocol, Protocol::Unknown);
        assert_eq!(host.hostname, "box");
    }

    #[test]
    fn rejects_files_that_are_not_sessions() {
        assert_eq!(
            parse_session("x", vec![], "[TERMINAL]\nType=xterm\n"),
            Err(ParseError::MissingConnection)
        );
        assert_eq!(
            parse_session("x", vec![], "[CONNECTION]\nProtocol=SSH\n"),
            Err(ParseError::MissingHost)
        );
    }

    #[test]
    fn walks_an_export_tree_into_folders() {
        let root = std::env::temp_dir().join(format!("harbour-xsh-{}", uuid::Uuid::new_v4()));
        let prod = root.join("Production").join("EU");
        std::fs::create_dir_all(&prod).unwrap();

        std::fs::write(root.join("jump.xsh"), XSHELL_6).unwrap();
        std::fs::write(
            prod.join("db-1.xsh"),
            "[CONNECTION]\nProtocol=SSH\nHost=db1.eu\nPort=22\n",
        )
        .unwrap();
        std::fs::write(root.join("notes.txt"), "not a session").unwrap();
        std::fs::write(root.join("broken.xsh"), "[TERMINAL]\nType=xterm\n").unwrap();

        let report = import_tree(&root).unwrap();
        std::fs::remove_dir_all(&root).ok();

        assert_eq!(report.hosts.len(), 2);
        // Root-level sessions sort before nested ones.
        assert_eq!(report.hosts[0].name, "jump");
        assert!(report.hosts[0].folder.is_empty());
        assert_eq!(report.hosts[1].name, "db-1");
        assert_eq!(report.hosts[1].folder, vec!["Production", "EU"]);

        assert_eq!(report.skipped.len(), 1);
        assert!(report.skipped[0].path.ends_with("broken.xsh"));
    }

    #[test]
    fn missing_export_directory_is_an_io_error() {
        let missing = std::env::temp_dir().join("harbour-does-not-exist-xsh");
        assert!(import_tree(&missing).is_err());
    }
}
