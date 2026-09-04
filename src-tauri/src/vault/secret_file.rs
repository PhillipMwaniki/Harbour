//! A file-based secret store, sealed under a master password.
//!
//! The OS keychain is Harbour's first choice for secrets ([`super::secrets`]).
//! But a headless Linux box has no Secret Service, and a copy of Harbour run
//! from a USB stick should not scatter secrets into whatever keychain the host
//! machine happens to have. For those, this: one file, sealed with the crypto
//! envelope under a master password the user sets, holding every secret in a
//! map keyed exactly as the keychain keys its entries, so the two are
//! interchangeable behind one interface.
//!
//! The master password is the only thing that opens the file. It is held in
//! memory - wrapped so it is wiped on drop - only while the store is unlocked,
//! and every write re-seals the whole file, so the plaintext never touches the
//! disk. A crash mid-write cannot corrupt the store: the new bytes land in a
//! temporary file that is renamed over the old one only once fully written.

use std::collections::BTreeMap;
use std::path::{Path, PathBuf};

use zeroize::Zeroizing;

use crate::crypto;
use crate::error::{AppError, AppResult};

/// The decrypted contents, present only while unlocked.
struct Unlocked {
    /// The master password, kept to re-seal the file on each write and wiped
    /// from memory when the store locks or drops.
    master: Zeroizing<String>,
    /// account -> secret. The account is the same string the keychain uses.
    secrets: BTreeMap<String, String>,
}

/// A master-password-protected secret file. Locked until [`unlock`](Self::unlock)
/// or [`create`](Self::create) is called.
pub struct SecretFile {
    path: PathBuf,
    unlocked: Option<Unlocked>,
}

impl SecretFile {
    /// A store backed by `path`. The file need not exist yet; the store starts
    /// locked either way.
    pub fn new(path: impl Into<PathBuf>) -> Self {
        Self {
            path: path.into(),
            unlocked: None,
        }
    }

    /// Whether a sealed file is already on disk. `create` is for when this is
    /// false; `unlock` for when it is true.
    pub fn exists(&self) -> bool {
        self.path.exists()
    }

    pub fn is_unlocked(&self) -> bool {
        self.unlocked.is_some()
    }

    /// Sets the master password for the first time, writing an empty sealed
    /// file. Refuses if one already exists, so an existing store is never
    /// silently replaced with an empty one.
    pub fn create(&mut self, master: &str) -> AppResult<()> {
        if master.is_empty() {
            return Err(AppError::Crypto("a master password cannot be empty".into()));
        }
        if self.exists() {
            return Err(AppError::Vault("a secret store already exists here".into()));
        }
        let unlocked = Unlocked {
            master: Zeroizing::new(master.to_string()),
            secrets: BTreeMap::new(),
        };
        write_sealed(&self.path, &unlocked)?;
        self.unlocked = Some(unlocked);
        Ok(())
    }

    /// Opens the file with `master` and loads it into memory. A wrong password
    /// and a tampered file are the same error.
    pub fn unlock(&mut self, master: &str) -> AppResult<()> {
        let bytes = std::fs::read(&self.path)?;
        let plaintext = crypto::open(master, &bytes)?;
        let secrets: BTreeMap<String, String> = serde_json::from_slice(&plaintext)
            .map_err(|_| AppError::Crypto("the secret store is not readable".into()))?;
        self.unlocked = Some(Unlocked {
            master: Zeroizing::new(master.to_string()),
            secrets,
        });
        Ok(())
    }

    /// Forgets the master password and the decrypted secrets, returning to the
    /// locked state. The file on disk is untouched.
    pub fn lock(&mut self) {
        self.unlocked = None;
    }

    /// Reads a secret. `Ok(None)` means there is no such entry; an error means
    /// the store is locked, which the caller must handle rather than treat as
    /// "no secret".
    pub fn get(&self, account: &str) -> AppResult<Option<String>> {
        let unlocked = self.require_unlocked()?;
        Ok(unlocked.secrets.get(account).cloned())
    }

    /// Stores a secret, re-sealing the file. Replaces any existing entry for
    /// the account.
    pub fn set(&mut self, account: &str, secret: &str) -> AppResult<()> {
        // Borrow the two fields disjointly: `unlocked` mutably, `path` shared.
        let unlocked = Self::unlocked_mut(&mut self.unlocked)?;
        unlocked
            .secrets
            .insert(account.to_string(), secret.to_string());
        write_sealed(&self.path, unlocked)
    }

    /// Removes a secret, re-sealing the file. Removing what is not there is
    /// success.
    pub fn delete(&mut self, account: &str) -> AppResult<()> {
        let unlocked = Self::unlocked_mut(&mut self.unlocked)?;
        if unlocked.secrets.remove(account).is_some() {
            write_sealed(&self.path, unlocked)?;
        }
        Ok(())
    }

    /// Re-seals the file under a new master password. The store must be
    /// unlocked; the old password is what unlocked it.
    pub fn change_master(&mut self, new_master: &str) -> AppResult<()> {
        if new_master.is_empty() {
            return Err(AppError::Crypto("a master password cannot be empty".into()));
        }
        let unlocked = Self::unlocked_mut(&mut self.unlocked)?;
        unlocked.master = Zeroizing::new(new_master.to_string());
        write_sealed(&self.path, unlocked)
    }

    fn require_unlocked(&self) -> AppResult<&Unlocked> {
        self.unlocked
            .as_ref()
            .ok_or_else(|| AppError::Vault("the secret store is locked".into()))
    }

    /// Borrows the unlocked state out of the field alone, so callers can hold
    /// `&self.path` at the same time.
    fn unlocked_mut(slot: &mut Option<Unlocked>) -> AppResult<&mut Unlocked> {
        slot.as_mut()
            .ok_or_else(|| AppError::Vault("the secret store is locked".into()))
    }
}

/// Seals the secrets and writes them, temp-file-then-rename so a crash cannot
/// leave a half-written store.
fn write_sealed(path: &Path, unlocked: &Unlocked) -> AppResult<()> {
    if let Some(parent) = path.parent() {
        std::fs::create_dir_all(parent)?;
    }
    let json = serde_json::to_vec(&unlocked.secrets)
        .map_err(|err| AppError::internal(format!("could not serialise secrets: {err}")))?;
    let sealed = crypto::seal(&unlocked.master, &json)?;

    let tmp = path.with_extension("tmp");
    std::fs::write(&tmp, sealed)?;
    std::fs::rename(&tmp, path)?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn temp_path() -> PathBuf {
        std::env::temp_dir().join(format!("harbour-secrets-{}.vault", uuid::Uuid::new_v4()))
    }

    /// Cleans up the file (and any stray temp) when the test ends.
    struct Cleanup(PathBuf);
    impl Drop for Cleanup {
        fn drop(&mut self) {
            std::fs::remove_file(&self.0).ok();
            std::fs::remove_file(self.0.with_extension("tmp")).ok();
        }
    }

    #[test]
    fn a_new_store_is_created_locked_then_holds_what_is_put_in_it() {
        let path = temp_path();
        let _cleanup = Cleanup(path.clone());

        let mut store = SecretFile::new(&path);
        assert!(!store.exists());
        assert!(!store.is_unlocked());

        store.create("master").unwrap();
        assert!(store.exists());
        assert!(store.is_unlocked());

        store.set("h1:password", "hunter2").unwrap();
        assert_eq!(
            store.get("h1:password").unwrap().as_deref(),
            Some("hunter2")
        );
        assert_eq!(store.get("h1:passphrase").unwrap(), None);
    }

    #[test]
    fn secrets_survive_a_lock_and_a_reopen_with_the_master_password() {
        let path = temp_path();
        let _cleanup = Cleanup(path.clone());

        {
            let mut store = SecretFile::new(&path);
            store.create("master").unwrap();
            store.set("h1:password", "hunter2").unwrap();
            store.set("h2:passphrase", "letmein").unwrap();
        }

        let mut reopened = SecretFile::new(&path);
        assert!(reopened.exists());
        reopened.unlock("master").unwrap();
        assert_eq!(
            reopened.get("h1:password").unwrap().as_deref(),
            Some("hunter2")
        );
        assert_eq!(
            reopened.get("h2:passphrase").unwrap().as_deref(),
            Some("letmein")
        );
    }

    #[test]
    fn the_wrong_master_password_will_not_open_it() {
        let path = temp_path();
        let _cleanup = Cleanup(path.clone());

        {
            let mut store = SecretFile::new(&path);
            store.create("right").unwrap();
            store.set("h1:password", "hunter2").unwrap();
        }

        let mut reopened = SecretFile::new(&path);
        let err = reopened.unlock("wrong").unwrap_err();
        assert_eq!(err.code(), "CRYPTO_ERROR");
        assert!(!reopened.is_unlocked());
    }

    #[test]
    fn a_locked_store_will_not_read_or_write() {
        let path = temp_path();
        let _cleanup = Cleanup(path.clone());

        let store = SecretFile::new(&path);
        // Reading a locked store is an error, not a silent "no secret".
        assert_eq!(store.get("h1:password").unwrap_err().code(), "VAULT_ERROR");
    }

    #[test]
    fn deleting_a_secret_removes_it_and_deleting_a_missing_one_is_fine() {
        let path = temp_path();
        let _cleanup = Cleanup(path.clone());

        let mut store = SecretFile::new(&path);
        store.create("master").unwrap();
        store.set("h1:password", "hunter2").unwrap();

        store.delete("h1:password").unwrap();
        assert_eq!(store.get("h1:password").unwrap(), None);
        // Removing what is already gone is success.
        store.delete("h1:password").unwrap();
    }

    #[test]
    fn the_master_password_can_be_changed_and_only_the_new_one_opens_it() {
        let path = temp_path();
        let _cleanup = Cleanup(path.clone());

        {
            let mut store = SecretFile::new(&path);
            store.create("old").unwrap();
            store.set("h1:password", "hunter2").unwrap();
            store.change_master("new").unwrap();
        }

        let mut reopened = SecretFile::new(&path);
        assert_eq!(reopened.unlock("old").unwrap_err().code(), "CRYPTO_ERROR");
        reopened.unlock("new").unwrap();
        assert_eq!(
            reopened.get("h1:password").unwrap().as_deref(),
            Some("hunter2")
        );
    }

    #[test]
    fn create_refuses_to_clobber_an_existing_store() {
        let path = temp_path();
        let _cleanup = Cleanup(path.clone());

        let mut store = SecretFile::new(&path);
        store.create("master").unwrap();

        let mut second = SecretFile::new(&path);
        assert_eq!(second.create("other").unwrap_err().code(), "VAULT_ERROR");
    }

    #[test]
    fn an_empty_master_password_is_refused() {
        let path = temp_path();
        let _cleanup = Cleanup(path.clone());

        let mut store = SecretFile::new(&path);
        assert_eq!(store.create("").unwrap_err().code(), "CRYPTO_ERROR");
    }

    /// The sealed file must not contain a secret in the clear.
    #[test]
    fn the_file_on_disk_is_ciphertext() {
        let path = temp_path();
        let _cleanup = Cleanup(path.clone());

        let mut store = SecretFile::new(&path);
        store.create("master").unwrap();
        store.set("h1:password", "hunter2-in-the-clear").unwrap();

        let bytes = std::fs::read(&path).unwrap();
        assert!(crypto::is_envelope(&bytes));
        // The secret does not appear anywhere in the file.
        assert!(!bytes.windows(20).any(|w| w == b"hunter2-in-the-clear"));
    }
}
