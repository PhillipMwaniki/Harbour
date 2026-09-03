//! Secrets, which live in the OS keychain and nowhere else.
//!
//! Windows Credential Manager, macOS Keychain, or the Secret Service on Linux,
//! whichever the platform has. The vault database holds only a flag saying
//! whether an entry is expected to exist - never a password, never a
//! passphrase, and never a hint that would narrow one down.
//!
//! There is deliberately no plaintext fallback. If the keychain is unavailable
//! the user is asked every time, which is an inconvenience; writing a password
//! to a file they did not ask to have written is a betrayal.

use keyring::Entry;

use crate::error::{AppError, AppResult};
use crate::vault::model::HostId;
use crate::vault::store::is_valid_id;

/// The service name every Harbour entry is filed under. Changing it orphans
/// every saved password, so it does not change.
const SERVICE: &str = "dev.harbour.app";

/// What a stored secret is for. A host may have both a password and a key
/// passphrase, so the kind is part of the key.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum SecretSlot {
    Password,
    KeyPassphrase,
}

impl SecretSlot {
    fn suffix(self) -> &'static str {
        match self {
            SecretSlot::Password => "password",
            SecretSlot::KeyPassphrase => "passphrase",
        }
    }
}

/// The keychain account name for one host's secret.
///
/// Host ids are UUIDs the store generated, but this is the one place where a
/// caller-supplied string would become an address in a shared namespace, so it
/// is checked rather than trusted.
fn account(host: &HostId, slot: SecretSlot) -> AppResult<String> {
    if !is_valid_id(host) {
        return Err(AppError::Vault(format!("`{host}` is not a valid host id")));
    }
    Ok(format!("{host}:{}", slot.suffix()))
}

fn entry(host: &HostId, slot: SecretSlot) -> AppResult<Entry> {
    let account = account(host, slot)?;
    Entry::new(SERVICE, &account).map_err(|err| AppError::Keyring(err.to_string()))
}

/// Reads a secret. `Ok(None)` means there is no entry, which is ordinary: the
/// user has simply never saved one.
pub fn get(host: &HostId, slot: SecretSlot) -> AppResult<Option<String>> {
    match entry(host, slot)?.get_password() {
        Ok(secret) => Ok(Some(secret)),
        Err(keyring::Error::NoEntry) => Ok(None),
        Err(err) => Err(AppError::Keyring(err.to_string())),
    }
}

pub fn set(host: &HostId, slot: SecretSlot, secret: &str) -> AppResult<()> {
    entry(host, slot)?
        .set_password(secret)
        .map_err(|err| AppError::Keyring(err.to_string()))
}

/// Removes a secret. Deleting one that is not there is success: the caller
/// asked for it to be gone, and it is.
pub fn delete(host: &HostId, slot: SecretSlot) -> AppResult<()> {
    match entry(host, slot)?.delete_credential() {
        Ok(()) | Err(keyring::Error::NoEntry) => Ok(()),
        Err(err) => Err(AppError::Keyring(err.to_string())),
    }
}

/// Whether this machine has a usable keychain at all.
///
/// Worth knowing before offering to save a password: a headless Linux box with
/// no Secret Service should not be told its password was remembered.
pub fn available() -> bool {
    Entry::store_status().is_ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The slot is part of the account name, so a host's password and its key
    /// passphrase cannot overwrite one another.
    #[test]
    fn each_slot_addresses_a_different_entry() {
        let host = uuid::Uuid::new_v4().to_string();
        let password = account(&host, SecretSlot::Password).unwrap();
        let passphrase = account(&host, SecretSlot::KeyPassphrase).unwrap();

        assert_ne!(password, passphrase);
        assert!(password.starts_with(&host));
        assert!(passphrase.starts_with(&host));
    }

    #[test]
    fn two_hosts_never_share_an_entry() {
        let a = uuid::Uuid::new_v4().to_string();
        let b = uuid::Uuid::new_v4().to_string();
        assert_ne!(
            account(&a, SecretSlot::Password).unwrap(),
            account(&b, SecretSlot::Password).unwrap()
        );
    }

    #[test]
    fn an_id_that_is_not_a_host_id_is_refused() {
        for bad in ["", "../../etc/passwd", "a b", "x:password"] {
            assert!(
                account(&bad.to_string(), SecretSlot::Password).is_err(),
                "{bad:?} should not be usable as a keychain key"
            );
        }
    }

    /// Exercises the real keychain, so it only runs where there is one. CI on
    /// Linux has no Secret Service, and macOS runners prompt.
    #[test]
    fn a_secret_round_trips_through_the_keychain() {
        if !available() {
            eprintln!("no keychain on this machine; skipping");
            return;
        }
        let host = uuid::Uuid::new_v4().to_string();

        assert_eq!(get(&host, SecretSlot::Password).unwrap(), None);

        set(&host, SecretSlot::Password, "hunter2").unwrap();
        assert_eq!(
            get(&host, SecretSlot::Password).unwrap().as_deref(),
            Some("hunter2")
        );

        // A second write replaces rather than duplicating.
        set(&host, SecretSlot::Password, "hunter3").unwrap();
        assert_eq!(
            get(&host, SecretSlot::Password).unwrap().as_deref(),
            Some("hunter3")
        );

        delete(&host, SecretSlot::Password).unwrap();
        assert_eq!(get(&host, SecretSlot::Password).unwrap(), None);

        // Deleting what is already gone is not an error.
        delete(&host, SecretSlot::Password).unwrap();
    }
}
