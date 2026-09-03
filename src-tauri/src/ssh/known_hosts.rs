//! The OpenSSH `known_hosts` store.
//!
//! Harbour reads the user's existing files - so a host already trusted from a
//! terminal is trusted here - but only ever *writes* to its own, as
//! `docs/security.md` requires. Nothing in this module prompts or decides
//! policy; it answers "what does the store say about this key?" and appends
//! what the user chose to trust.

use std::fs::{File, OpenOptions};
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};

use data_encoding::BASE64;
use hmac::{Hmac, KeyInit, Mac};
use russh::keys::ssh_key::{HashAlg, PublicKey};
use serde::Serialize;
use sha1::Sha1;

/// A host key recorded in the store, in the form the UI shows it.
#[derive(Debug, Clone, PartialEq, Eq, Serialize)]
#[serde(rename_all = "camelCase")]
pub struct StoredKey {
    /// SSH algorithm name, e.g. `ssh-ed25519`.
    pub algorithm: String,
    /// `SHA256:...`, the same form OpenSSH and the UI show.
    pub fingerprint: String,
    /// Where it was found, for a prompt that has to explain itself.
    pub source: String,
    pub line: usize,
}

/// What the store knows about the key a server just offered.
#[derive(Debug, Clone, PartialEq, Eq)]
pub enum Verdict {
    /// Exactly this key is recorded for this host.
    Trusted,
    /// Nothing is recorded for this host and algorithm. `other` lists keys
    /// held for the host under a *different* algorithm, which is worth showing
    /// - it is the difference between "new host" and "new key type".
    Unknown { other: Vec<StoredKey> },
    /// A key of the same algorithm is recorded and it is not this one. This is
    /// the case that defaults to reject.
    Changed { stored: Vec<StoredKey> },
    /// The key is explicitly marked `@revoked`. Never connectable.
    Revoked,
}

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
enum Marker {
    None,
    CertAuthority,
    Revoked,
}

/// One parsed `known_hosts` line.
struct Entry {
    marker: Marker,
    patterns: String,
    key: PublicKey,
    line: usize,
}

/// The set of files Harbour consults, and the one it appends to.
#[derive(Debug, Clone)]
pub struct KnownHosts {
    read_paths: Vec<PathBuf>,
    write_path: PathBuf,
}

impl KnownHosts {
    /// `write_path` is Harbour's own file; the user's OpenSSH files are read
    /// as well, and are never modified.
    pub fn new(write_path: PathBuf) -> Self {
        let mut read_paths = Vec::new();
        if let Some(home) = dirs::home_dir() {
            read_paths.push(home.join(".ssh").join("known_hosts"));
            read_paths.push(home.join(".ssh").join("known_hosts2"));
        }
        read_paths.push(write_path.clone());
        Self {
            read_paths,
            write_path,
        }
    }

    /// A store backed by explicit paths. Used by the tests, and by anything
    /// that needs to bypass the user's real files.
    pub fn with_paths(read_paths: Vec<PathBuf>, write_path: PathBuf) -> Self {
        Self {
            read_paths,
            write_path,
        }
    }

    pub fn write_path(&self) -> &Path {
        &self.write_path
    }

    /// Classifies `offered` against everything recorded for `host:port`.
    pub fn verify(&self, host: &str, port: u16, offered: &PublicKey) -> Verdict {
        let target = host_pattern(host, port);
        let mut same_algorithm = Vec::new();
        let mut other_algorithm = Vec::new();

        for path in &self.read_paths {
            let source = path.display().to_string();
            for entry in read_entries(path) {
                if !matches_host(&entry.patterns, &target) {
                    continue;
                }
                match entry.marker {
                    // A certificate authority says nothing about a plain host
                    // key; validating offered certificates is milestone 9.
                    Marker::CertAuthority => continue,
                    Marker::Revoked if entry.key == *offered => return Verdict::Revoked,
                    Marker::Revoked => continue,
                    Marker::None => {}
                }
                if entry.key == *offered {
                    return Verdict::Trusted;
                }
                let stored = StoredKey {
                    algorithm: entry.key.algorithm().to_string(),
                    fingerprint: fingerprint(&entry.key),
                    source: source.clone(),
                    line: entry.line,
                };
                if entry.key.algorithm() == offered.algorithm() {
                    same_algorithm.push(stored);
                } else {
                    other_algorithm.push(stored);
                }
            }
        }

        if same_algorithm.is_empty() {
            Verdict::Unknown {
                other: other_algorithm,
            }
        } else {
            Verdict::Changed {
                stored: same_algorithm,
            }
        }
    }

    /// Appends `key` to Harbour's own file. The user's files stay untouched.
    pub fn learn(&self, host: &str, port: u16, key: &PublicKey) -> std::io::Result<()> {
        if let Some(parent) = self.write_path.parent() {
            std::fs::create_dir_all(parent)?;
        }
        let encoded = key
            .to_openssh()
            .map_err(|err| std::io::Error::new(std::io::ErrorKind::InvalidData, err))?;
        let line = format!("{} {}\n", host_pattern(host, port), encoded);

        let mut options = OpenOptions::new();
        options.create(true).append(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.mode(0o600);
        }
        options.open(&self.write_path)?.write_all(line.as_bytes())
    }
}

/// `SHA256:...`, matching what `ssh-keygen -l` prints.
pub fn fingerprint(key: &PublicKey) -> String {
    key.fingerprint(HashAlg::Sha256).to_string()
}

/// How a host is written in `known_hosts`: bare for port 22, bracketed
/// otherwise. Hashed entries hash exactly this string.
fn host_pattern(host: &str, port: u16) -> String {
    if port == 22 {
        host.to_string()
    } else {
        format!("[{host}]:{port}")
    }
}

/// Reads and parses one file. A malformed line is skipped rather than
/// poisoning the file: entries whose key type we cannot parse are common in
/// real files, and none of them is a reason to stop trusting the rest.
fn read_entries(path: &Path) -> Vec<Entry> {
    let Ok(file) = File::open(path) else {
        return Vec::new();
    };
    BufReader::new(file)
        .lines()
        .enumerate()
        .filter_map(|(index, line)| parse_line(&line.ok()?, index + 1))
        .collect()
}

fn parse_line(line: &str, number: usize) -> Option<Entry> {
    let line = line.trim();
    if line.is_empty() || line.starts_with('#') {
        return None;
    }

    let mut fields = line.split_whitespace();
    let mut first = fields.next()?;
    let marker = match first {
        "@cert-authority" => {
            first = fields.next()?;
            Marker::CertAuthority
        }
        "@revoked" => {
            first = fields.next()?;
            Marker::Revoked
        }
        _ => Marker::None,
    };

    let algorithm = fields.next()?;
    let encoded = fields.next()?;
    let key = PublicKey::from_openssh(&format!("{algorithm} {encoded}")).ok()?;

    Some(Entry {
        marker,
        patterns: first.to_string(),
        key,
        line: number,
    })
}

/// OpenSSH host matching: a comma-separated list of patterns, any of which may
/// be hashed, negated, or contain `*` and `?` wildcards. A negated pattern that
/// matches vetoes the whole entry.
fn matches_host(patterns: &str, target: &str) -> bool {
    // Hashed entries cannot be wildcards - a hash matches one host exactly - so
    // they are checked here and everything else is ordinary pattern matching.
    let (hashed, plain): (Vec<&str>, Vec<&str>) =
        patterns.split(',').partition(|p| p.starts_with("|1|"));

    if hashed.iter().any(|pattern| hashed_matches(pattern, target)) {
        return true;
    }
    !plain.is_empty() && crate::glob::matches_list(&plain.join(","), target)
}

/// `|1|<base64 salt>|<base64 HMAC-SHA1>`; the MAC is keyed with the salt and
/// taken over the host string.
fn hashed_matches(pattern: &str, target: &str) -> bool {
    let mut parts = pattern.split('|').skip(2);
    let (Some(salt), Some(hash)) = (parts.next(), parts.next()) else {
        return false;
    };
    let (Ok(salt), Ok(hash)) = (
        BASE64.decode(salt.as_bytes()),
        BASE64.decode(hash.as_bytes()),
    ) else {
        return false;
    };
    let Ok(mac) = Hmac::<Sha1>::new_from_slice(&salt) else {
        return false;
    };
    mac.chain_update(target.as_bytes())
        .verify_slice(&hash)
        .is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::{AtomicUsize, Ordering};

    const KEY_A: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIGuxS7NMEyMPvug2pbsnVUW0cGDyAqt8HlYq7Qudlj7A";
    const KEY_B: &str =
        "ssh-ed25519 AAAAC3NzaC1lZDI1NTE5AAAAIPiUdw+LQoRUxhSMeP52WjCu2szRCnp2lcJ+xROivTLS";
    const KEY_RSA: &str = "ssh-rsa AAAAB3NzaC1yc2EAAAADAQABAAABAQCx8ZRouyQLMSc9ZEG/x6aJQkm4ya69dsMLjcEa+HR5h2zPblYWfBWMfMX709vSH9DO9nfl7cqcnXAtkt4A5iETwY0x5j8qnVBoY2CKxgjV0uNw+DMj6m+m4n6ot8ieabnRm+115DJPDxQcaBbY4mFY5835b0Il+w9ttyg6S35P3ULRxyji9uH4cJGgpaPuidG1VzaDHZS5/LMWDsM/KSeIUBFEgHs/cvGJHSThq0lEoDbUJrZTFgTKBgXsq0/Emh5jTyPasEkCG++ZvozbSNivatasn8Ql4xn1EaUoJjDCEXn+MB2KgsD/95ojx+pgNtGMYczTFFe8h7UFfzWBDpt3";

    /// `ssh-keygen -H` output for `example.com` and `[example.com]:2222`,
    /// so the hashed-host path is tested against real OpenSSH hashes rather
    /// than ones this module produced itself.
    const HASHED_22: &str = "|1|Kj1vfNVimDSOvH8HQ0RP/Y5/Vxc=|9IhC1akvwloczzaEaSzx9IOpe+o=";
    const HASHED_2222: &str = "|1|VXQF8iLtRMhyEQ7Xb7Seg7qUdzk=|eUkRWFBfNb9raIhd1Jg3ImgSDoU=";

    fn key(openssh: &str) -> PublicKey {
        PublicKey::from_openssh(openssh).expect("test key should parse")
    }

    /// A store whose only file holds `contents`, plus an empty writable file.
    fn store(contents: &str) -> (KnownHosts, PathBuf) {
        static COUNTER: AtomicUsize = AtomicUsize::new(0);
        let dir = std::env::temp_dir().join(format!(
            "harbour-kh-{}-{}",
            std::process::id(),
            COUNTER.fetch_add(1, Ordering::Relaxed)
        ));
        std::fs::create_dir_all(&dir).unwrap();
        let read = dir.join("known_hosts");
        std::fs::write(&read, contents).unwrap();
        let write = dir.join("harbour_known_hosts");
        (KnownHosts::with_paths(vec![read], write.clone()), write)
    }

    #[test]
    fn an_exact_match_is_trusted() {
        let (hosts, _) = store(&format!("example.com {KEY_A}\n"));
        assert_eq!(
            hosts.verify("example.com", 22, &key(KEY_A)),
            Verdict::Trusted
        );
    }

    #[test]
    fn an_unrecorded_host_is_unknown() {
        let (hosts, _) = store(&format!("example.com {KEY_A}\n"));
        let Verdict::Unknown { other } = hosts.verify("other.example", 22, &key(KEY_A)) else {
            panic!("an unrecorded host must be unknown");
        };
        assert!(other.is_empty());
    }

    #[test]
    fn a_different_key_of_the_same_type_has_changed() {
        let (hosts, _) = store(&format!("example.com {KEY_A}\n"));
        let Verdict::Changed { stored } = hosts.verify("example.com", 22, &key(KEY_B)) else {
            panic!("a substituted key must read as changed");
        };
        assert_eq!(stored.len(), 1);
        assert_eq!(stored[0].fingerprint, fingerprint(&key(KEY_A)));
        assert_eq!(stored[0].line, 1);
    }

    /// A host that only has an RSA key on file offering an ed25519 one is a
    /// new key *type*, not a substituted key - it must not raise the alarm.
    #[test]
    fn a_key_of_another_type_is_unknown_but_reported() {
        let (hosts, _) = store(&format!("example.com {KEY_RSA}\n"));
        let Verdict::Unknown { other } = hosts.verify("example.com", 22, &key(KEY_A)) else {
            panic!("a new key type must not read as changed");
        };
        assert_eq!(other.len(), 1);
        assert_eq!(other[0].algorithm, "ssh-rsa");
    }

    #[test]
    fn a_revoked_key_is_never_trusted() {
        let (hosts, _) = store(&format!("@revoked example.com {KEY_A}\n"));
        assert_eq!(
            hosts.verify("example.com", 22, &key(KEY_A)),
            Verdict::Revoked
        );
    }

    /// A revocation covers the key it names, not every key for that host.
    #[test]
    fn revoking_one_key_leaves_another_trusted() {
        let (hosts, _) = store(&format!(
            "@revoked example.com {KEY_B}\nexample.com {KEY_A}\n"
        ));
        assert_eq!(
            hosts.verify("example.com", 22, &key(KEY_A)),
            Verdict::Trusted
        );
        assert_eq!(
            hosts.verify("example.com", 22, &key(KEY_B)),
            Verdict::Revoked
        );
    }

    /// A CA line is not a host key, and must not be mistaken for a changed one.
    #[test]
    fn a_cert_authority_line_is_ignored() {
        let (hosts, _) = store(&format!("@cert-authority *.example.com {KEY_A}\n"));
        let Verdict::Unknown { other } = hosts.verify("host.example.com", 22, &key(KEY_B)) else {
            panic!("a CA entry says nothing about a host key");
        };
        assert!(other.is_empty());
    }

    #[test]
    fn hashed_entries_match_on_both_port_forms() {
        let (hosts, _) = store(&format!("{HASHED_22} {KEY_A}\n{HASHED_2222} {KEY_B}\n"));
        assert_eq!(
            hosts.verify("example.com", 22, &key(KEY_A)),
            Verdict::Trusted
        );
        assert_eq!(
            hosts.verify("example.com", 2222, &key(KEY_B)),
            Verdict::Trusted
        );
    }

    /// The port is part of the identity: port 2222 must not inherit the trust
    /// recorded for port 22.
    #[test]
    fn a_nonstandard_port_is_a_different_host() {
        let (hosts, _) = store(&format!("example.com {KEY_A}\n"));
        assert!(matches!(
            hosts.verify("example.com", 2222, &key(KEY_A)),
            Verdict::Unknown { .. }
        ));
    }

    #[test]
    fn wildcard_patterns_match_and_negations_veto() {
        let (hosts, _) = store(&format!("*.example.com,!admin.example.com {KEY_A}\n"));
        assert_eq!(
            hosts.verify("web.example.com", 22, &key(KEY_A)),
            Verdict::Trusted
        );
        assert!(matches!(
            hosts.verify("admin.example.com", 22, &key(KEY_A)),
            Verdict::Unknown { .. }
        ));
    }

    /// Real files carry comments, blank lines and key types we may not
    /// support. None of that may stop the rest of the file being read.
    #[test]
    fn malformed_lines_do_not_hide_valid_ones() {
        let contents = format!(
            "# a comment\n\nexample.com ssh-rsa not-actually-base64\ngarbage\nexample.com {KEY_A}\n"
        );
        let (hosts, _) = store(&contents);
        assert_eq!(
            hosts.verify("example.com", 22, &key(KEY_A)),
            Verdict::Trusted
        );
    }

    #[test]
    fn learning_a_key_makes_it_trusted() {
        let (hosts, write_path) = store("");
        assert!(matches!(
            hosts.verify("example.com", 2222, &key(KEY_A)),
            Verdict::Unknown { .. }
        ));

        hosts.learn("example.com", 2222, &key(KEY_A)).unwrap();

        let written = std::fs::read_to_string(&write_path).unwrap();
        assert!(written.starts_with("[example.com]:2222 ssh-ed25519 "));

        // The file just written is also a read source, so the verdict flips.
        let reread = KnownHosts::with_paths(vec![write_path.clone()], write_path);
        assert_eq!(
            reread.verify("example.com", 2222, &key(KEY_A)),
            Verdict::Trusted
        );
    }
}
