//! The cryptography Harbour uses to protect a vault at rest and in an export.
//!
//! One primitive, used two ways. A passphrase is stretched with Argon2id and
//! the result seals bytes with XChaCha20-Poly1305; the salt, nonce and
//! parameters travel with the ciphertext in a self-describing envelope, so a
//! file carries everything needed to open it except the passphrase. The
//! master-password feature and encrypted export/import are both this, applied
//! to different bytes.
//!
//! What is deliberately not here: any cleverness. The parameters are the
//! recommended ones, the construction is the boring authenticated one, and the
//! format carries a version byte so a future change is a new branch rather than
//! a silent reinterpretation of old files.

use argon2::{Algorithm, Argon2, Params, Version};
use chacha20poly1305::aead::Aead;
use chacha20poly1305::{Key, KeyInit, XChaCha20Poly1305, XNonce};
use zeroize::Zeroize;

use crate::error::{AppError, AppResult};

/// Every envelope begins with this, so a file that is not one is refused
/// before anything is derived from a passphrase.
const MAGIC: &[u8; 6] = b"HBRVLT";
/// Bumped only when the envelope layout changes. New parameters do not need
/// it - they are already recorded in each envelope.
const FORMAT_VERSION: u8 = 1;

const SALT_LEN: usize = 16;
const NONCE_LEN: usize = 24; // XChaCha20-Poly1305
const KEY_LEN: usize = 32;

/// Argon2id cost. 64 MiB and three passes is the interactive recommendation:
/// enough to make a guess expensive, not so much that unlocking the vault
/// stalls. Stored in the envelope so a file sealed today still opens if these
/// change tomorrow.
const ARGON_MEM_KIB: u32 = 64 * 1024;
const ARGON_TIME: u32 = 3;
const ARGON_LANES: u32 = 1;

/// A key derived from a passphrase, wiped from memory on drop.
struct DerivedKey([u8; KEY_LEN]);

impl Drop for DerivedKey {
    fn drop(&mut self) {
        self.0.zeroize();
    }
}

/// The Argon2 parameters used for one envelope. Carried in the header so a
/// change to the defaults never orphans an existing file.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
struct KdfParams {
    mem_kib: u32,
    time: u32,
    lanes: u32,
}

impl KdfParams {
    fn current() -> Self {
        Self {
            mem_kib: ARGON_MEM_KIB,
            time: ARGON_TIME,
            lanes: ARGON_LANES,
        }
    }

    fn derive(self, passphrase: &[u8], salt: &[u8]) -> AppResult<DerivedKey> {
        let params = Params::new(self.mem_kib, self.time, self.lanes, Some(KEY_LEN))
            .map_err(|err| AppError::Crypto(format!("bad key parameters: {err}")))?;
        let argon = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);
        let mut key = [0u8; KEY_LEN];
        argon
            .hash_password_into(passphrase, salt, &mut key)
            .map_err(|err| AppError::Crypto(format!("could not derive a key: {err}")))?;
        Ok(DerivedKey(key))
    }
}

fn random(bytes: &mut [u8]) -> AppResult<()> {
    getrandom::fill(bytes).map_err(|err| AppError::Crypto(format!("no system randomness: {err}")))
}

/// Seals `plaintext` under `passphrase`, returning a self-describing envelope.
///
/// The layout is `MAGIC (6) | version (1) | mem_kib (4, LE) | time (4, LE) |
/// lanes (4, LE) | salt (16) | nonce (24) | ciphertext+tag`. Everything but
/// the passphrase is in here, so the file is enough to open itself.
pub fn seal(passphrase: &str, plaintext: &[u8]) -> AppResult<Vec<u8>> {
    let params = KdfParams::current();
    let mut salt = [0u8; SALT_LEN];
    let mut nonce = [0u8; NONCE_LEN];
    random(&mut salt)?;
    random(&mut nonce)?;

    let key = params.derive(passphrase.as_bytes(), &salt)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key.0));
    let ciphertext = cipher
        .encrypt(XNonce::from_slice(&nonce), plaintext)
        .map_err(|_| AppError::Crypto("encryption failed".into()))?;

    let mut out =
        Vec::with_capacity(MAGIC.len() + 1 + 12 + SALT_LEN + NONCE_LEN + ciphertext.len());
    out.extend_from_slice(MAGIC);
    out.push(FORMAT_VERSION);
    out.extend_from_slice(&params.mem_kib.to_le_bytes());
    out.extend_from_slice(&params.time.to_le_bytes());
    out.extend_from_slice(&params.lanes.to_le_bytes());
    out.extend_from_slice(&salt);
    out.extend_from_slice(&nonce);
    out.extend_from_slice(&ciphertext);
    Ok(out)
}

/// The header length: magic, version, three u32 parameters, salt and nonce.
const HEADER_LEN: usize = 6 + 1 + 12 + SALT_LEN + NONCE_LEN;

/// Opens an envelope produced by [`seal`].
///
/// A wrong passphrase and a tampered file are the same error - the tag simply
/// fails to verify - which is what we want: neither reveals which it was.
pub fn open(passphrase: &str, envelope: &[u8]) -> AppResult<Vec<u8>> {
    if envelope.len() < HEADER_LEN {
        return Err(AppError::Crypto("not a Harbour vault file".into()));
    }
    if &envelope[..6] != MAGIC {
        return Err(AppError::Crypto("not a Harbour vault file".into()));
    }
    let version = envelope[6];
    if version != FORMAT_VERSION {
        return Err(AppError::Crypto(format!(
            "this file is version {version}; this Harbour reads version {FORMAT_VERSION}"
        )));
    }

    let u32_at = |offset: usize| {
        u32::from_le_bytes([
            envelope[offset],
            envelope[offset + 1],
            envelope[offset + 2],
            envelope[offset + 3],
        ])
    };
    let params = KdfParams {
        mem_kib: u32_at(7),
        time: u32_at(11),
        lanes: u32_at(15),
    };
    let salt = &envelope[19..19 + SALT_LEN];
    let nonce = &envelope[19 + SALT_LEN..HEADER_LEN];
    let ciphertext = &envelope[HEADER_LEN..];

    let key = params.derive(passphrase.as_bytes(), salt)?;
    let cipher = XChaCha20Poly1305::new(Key::from_slice(&key.0));
    cipher
        .decrypt(XNonce::from_slice(nonce), ciphertext)
        .map_err(|_| AppError::Crypto("wrong passphrase, or the file has been altered".into()))
}

/// Whether `bytes` begins with the envelope magic - a cheap check before
/// asking the user for a passphrase.
pub fn is_envelope(bytes: &[u8]) -> bool {
    bytes.len() >= MAGIC.len() && &bytes[..MAGIC.len()] == MAGIC
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_sealed_message_opens_with_the_right_passphrase() {
        let sealed = seal("correct horse", b"battery staple").unwrap();
        assert!(is_envelope(&sealed));
        assert_eq!(open("correct horse", &sealed).unwrap(), b"battery staple");
    }

    #[test]
    fn the_wrong_passphrase_is_refused() {
        let sealed = seal("right", b"secret hosts").unwrap();
        let err = open("wrong", &sealed).unwrap_err();
        assert_eq!(err.code(), "CRYPTO_ERROR");
    }

    #[test]
    fn a_flipped_byte_fails_to_verify() {
        let mut sealed = seal("pw", b"payload").unwrap();
        // Flip a byte in the ciphertext; the authentication tag must catch it.
        let last = sealed.len() - 1;
        sealed[last] ^= 0x01;
        assert!(open("pw", &sealed).is_err());
    }

    #[test]
    fn a_truncated_or_foreign_file_is_not_mistaken_for_ours() {
        assert!(open("pw", b"").is_err());
        assert!(open("pw", b"too short").is_err());
        assert!(open("pw", &[0u8; HEADER_LEN + 4]).is_err());
        assert!(!is_envelope(b"PK\x03\x04 a zip"));
    }

    #[test]
    fn every_seal_is_different_even_for_the_same_input() {
        // Fresh salt and nonce each time, so two seals of one message do not
        // reveal that they are the same message.
        let a = seal("pw", b"same").unwrap();
        let b = seal("pw", b"same").unwrap();
        assert_ne!(a, b);
        assert_eq!(open("pw", &a).unwrap(), open("pw", &b).unwrap());
    }

    #[test]
    fn the_parameters_travel_with_the_file() {
        // An envelope sealed with today's parameters records them, so it opens
        // even if the defaults change later. Simulate by reading them back.
        let sealed = seal("pw", b"x").unwrap();
        assert_eq!(&sealed[..6], MAGIC);
        assert_eq!(sealed[6], FORMAT_VERSION);
        let mem = u32::from_le_bytes([sealed[7], sealed[8], sealed[9], sealed[10]]);
        assert_eq!(mem, ARGON_MEM_KIB);
    }

    #[test]
    fn an_empty_payload_round_trips() {
        let sealed = seal("pw", b"").unwrap();
        assert_eq!(open("pw", &sealed).unwrap(), b"");
    }
}
