//! Generating an SSH keypair, and the command that installs its public half on
//! a host.
//!
//! The private key is written locally and never leaves the machine; only the
//! public key is ever sent anywhere. Deployment reuses the one-shot `exec` path
//! ([`crate::ssh::client::run_command`]) with an idempotent shell command, so it
//! follows a jump chain and never appends a key twice.

use std::path::{Path, PathBuf};

use russh::keys::ssh_key::{Algorithm, LineEnding, PrivateKey};

use crate::error::{AppError, AppResult};
use crate::ssh::known_hosts;

/// A freshly generated keypair on disk.
#[derive(Debug, Clone)]
pub struct Generated {
    /// The private key path.
    pub path: String,
    /// The public key path (`<path>.pub`).
    pub public_path: String,
    /// The public key in OpenSSH one-line form - what gets deployed.
    pub public_key: String,
    /// `SHA256:...`, as the connect-time prompt shows it.
    pub fingerprint: String,
}

/// Generates an Ed25519 keypair at `path`, optionally encrypting the private
/// key with `passphrase`, and writes both files.
///
/// Refuses to overwrite an existing key: a keypair is not something to clobber
/// by accident. `comment` is the trailing label on the public key line, for
/// recognising it later in an `authorized_keys`.
pub fn generate(path: &Path, passphrase: Option<&str>, comment: &str) -> AppResult<Generated> {
    if path.exists() {
        return Err(AppError::internal(format!(
            "{} already exists; choose another name",
            path.display()
        )));
    }

    let mut key = PrivateKey::random(&mut rand::rng(), Algorithm::Ed25519)
        .map_err(|err| AppError::internal(format!("could not generate a key: {err}")))?;
    key.set_comment(comment);

    // The on-disk private key is encrypted when a passphrase is given; the
    // public key and the fingerprint come from the unencrypted key in memory.
    let to_store = match passphrase.filter(|p| !p.is_empty()) {
        Some(passphrase) => key
            .encrypt(&mut rand::rng(), passphrase)
            .map_err(|err| AppError::internal(format!("could not encrypt the key: {err}")))?,
        None => key.clone(),
    };

    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let openssh = to_store
        .to_openssh(LineEnding::LF)
        .map_err(|err| AppError::internal(format!("could not encode the key: {err}")))?;
    std::fs::write(path, openssh.as_bytes())?;

    let public = key.public_key();
    let public_key = public
        .to_openssh()
        .map_err(|err| AppError::internal(format!("could not encode the public key: {err}")))?;
    let public_path = pub_path(path);
    std::fs::write(&public_path, format!("{public_key}\n"))?;

    // A private key must not be world-readable; the public key may be.
    restrict(path, 0o600)?;
    restrict(&public_path, 0o644)?;

    Ok(Generated {
        path: path.display().to_string(),
        public_path: public_path.display().to_string(),
        fingerprint: known_hosts::fingerprint(public),
        public_key,
    })
}

/// `<path>.pub`, the conventional name for the public half.
fn pub_path(path: &Path) -> PathBuf {
    let mut name = path.as_os_str().to_owned();
    name.push(".pub");
    PathBuf::from(name)
}

#[cfg(unix)]
fn restrict(path: &Path, mode: u32) -> AppResult<()> {
    use std::os::unix::fs::PermissionsExt;
    std::fs::set_permissions(path, std::fs::Permissions::from_mode(mode)).map_err(Into::into)
}

#[cfg(not(unix))]
fn restrict(_path: &Path, _mode: u32) -> AppResult<()> {
    // Windows uses ACLs, not mode bits; the file inherits the user's profile
    // permissions, which are already per-user.
    Ok(())
}

/// The shell command that installs `public_key` into the remote
/// `~/.ssh/authorized_keys`, idempotently and with the right permissions.
///
/// Wrapped in `/bin/sh -c` so it runs the same whatever the login shell is
/// (fish included), and printing a marker so the caller can tell an install
/// from a no-op. The key is validated to contain neither a quote nor a newline
/// before it is interpolated, so the single-quoted wrapper cannot be broken
/// out of - a public key never contains either.
pub fn install_command(public_key: &str) -> AppResult<String> {
    let key = public_key.trim();
    if key.is_empty() || key.contains(['\'', '"', '\n', '\r']) {
        return Err(AppError::internal("that does not look like a public key"));
    }
    Ok(format!(
        "/bin/sh -c 'K=\"{key}\"; \
         mkdir -p \"$HOME/.ssh\" && chmod 700 \"$HOME/.ssh\" && \
         touch \"$HOME/.ssh/authorized_keys\" && chmod 600 \"$HOME/.ssh/authorized_keys\" && \
         grep -qxF \"$K\" \"$HOME/.ssh/authorized_keys\" && echo HARBOUR_PRESENT || \
         {{ printf \"%s\\n\" \"$K\" >> \"$HOME/.ssh/authorized_keys\" && echo HARBOUR_ADDED; }}'"
    ))
}

/// What the install command reported.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Installed {
    /// The key was added to `authorized_keys`.
    Added,
    /// The key was already there; nothing changed.
    AlreadyPresent,
}

/// Reads the marker the install command printed out of its stdout.
pub fn read_marker(stdout: &str) -> AppResult<Installed> {
    if stdout.contains("HARBOUR_PRESENT") {
        Ok(Installed::AlreadyPresent)
    } else if stdout.contains("HARBOUR_ADDED") {
        Ok(Installed::Added)
    } else {
        Err(AppError::internal(
            "the key could not be installed on the host",
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn scratch() -> PathBuf {
        let dir = std::env::temp_dir().join(format!("harbour-keygen-{}", uuid::Uuid::new_v4()));
        std::fs::create_dir_all(&dir).unwrap();
        dir
    }

    #[test]
    fn generates_a_valid_keypair_with_a_fingerprint() {
        let dir = scratch();
        let path = dir.join("id_ed25519");
        let generated = generate(&path, None, "harbour@test").unwrap();

        assert!(path.exists());
        assert!(dir.join("id_ed25519.pub").exists());
        assert!(generated.public_key.starts_with("ssh-ed25519 "));
        assert!(generated.public_key.contains("harbour@test"));
        assert!(generated.fingerprint.starts_with("SHA256:"));

        // The written private key parses back.
        let text = std::fs::read_to_string(&path).unwrap();
        assert!(PrivateKey::from_openssh(&text).is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn an_encrypted_key_needs_its_passphrase() {
        let dir = scratch();
        let path = dir.join("id_ed25519");
        generate(&path, Some("hunter2"), "harbour@test").unwrap();

        let text = std::fs::read_to_string(&path).unwrap();
        let key = PrivateKey::from_openssh(&text).unwrap();
        assert!(key.is_encrypted());
        assert!(key.decrypt("wrong").is_err());
        assert!(key.decrypt("hunter2").is_ok());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn it_will_not_overwrite_an_existing_key() {
        let dir = scratch();
        let path = dir.join("id_ed25519");
        generate(&path, None, "harbour@test").unwrap();
        assert!(generate(&path, None, "harbour@test").is_err());
        std::fs::remove_dir_all(&dir).ok();
    }

    #[cfg(unix)]
    #[test]
    fn the_private_key_is_not_world_readable() {
        use std::os::unix::fs::PermissionsExt;
        let dir = scratch();
        let path = dir.join("id_ed25519");
        generate(&path, None, "harbour@test").unwrap();
        let mode = std::fs::metadata(&path).unwrap().permissions().mode();
        assert_eq!(mode & 0o077, 0, "group/other bits must be clear");
        std::fs::remove_dir_all(&dir).ok();
    }

    #[test]
    fn the_install_command_embeds_the_key_and_is_idempotent_shaped() {
        let cmd = install_command("ssh-ed25519 AAAAC3Nz harbour@test").unwrap();
        assert!(cmd.contains("ssh-ed25519 AAAAC3Nz harbour@test"));
        assert!(cmd.contains("grep -qxF"));
        assert!(cmd.contains("HARBOUR_PRESENT"));
        assert!(cmd.contains("HARBOUR_ADDED"));
    }

    #[test]
    fn a_key_that_could_break_the_quoting_is_refused() {
        assert!(install_command("has'quote").is_err());
        assert!(install_command("has\nnewline").is_err());
        assert!(install_command("  ").is_err());
    }

    #[test]
    fn the_marker_says_added_or_present() {
        assert_eq!(read_marker("HARBOUR_ADDED\n").unwrap(), Installed::Added);
        assert_eq!(
            read_marker("HARBOUR_PRESENT\n").unwrap(),
            Installed::AlreadyPresent
        );
        assert!(read_marker("something went wrong").is_err());
    }
}
