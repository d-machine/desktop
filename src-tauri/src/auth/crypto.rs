use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use argon2::{Algorithm, Argon2, Params, Version};
use base64::{engine::general_purpose::STANDARD as B64, Engine};
use rand::{rngs::OsRng, RngCore};
use serde::{Deserialize, Serialize};
use zeroize::Zeroize;
use anyhow::{anyhow, Result};

/// Argon2id parameters — tuned for ~100ms on a mid-range CPU.
/// Increase m_cost for stronger security at the cost of unlock speed.
const ARGON2_M_COST: u32 = 65536; // 64 MB
const ARGON2_T_COST: u32 = 3;
const ARGON2_P_COST: u32 = 4;
const KEY_LEN: usize = 32; // 256-bit
const NONCE_LEN: usize = 12; // 96-bit GCM nonce
const SALT_LEN: usize = 16; // 128-bit Argon2 salt

/// A wrapped (encrypted) copy of the master key, along with the salt needed to
/// re-derive the wrapping key from a PIN or passphrase.
#[derive(Serialize, Deserialize, Clone)]
pub struct WrappedKey {
    pub salt: String,           // base64 Argon2 salt
    pub nonce: String,          // base64 AES-GCM nonce
    pub ciphertext: String,     // base64 AES-GCM ciphertext of master key
}

/// The auth config persisted in `auth.json` in the app data directory.
#[derive(Serialize, Deserialize)]
pub struct AuthConfig {
    pub version: u8,
    pub pin_wrapped: WrappedKey,        // master key wrapped with PIN
}

/// The recovery file (`.ptbak`) the user saves externally.
#[derive(Serialize, Deserialize)]
pub struct RecoveryFile {
    pub version: u8,
    pub passphrase_wrapped: WrappedKey, // master key wrapped with passphrase
}

/// Generate a cryptographically random 32-byte master key.
pub fn generate_master_key() -> [u8; KEY_LEN] {
    let mut key = [0u8; KEY_LEN];
    OsRng.fill_bytes(&mut key);
    key
}

/// Derive a 256-bit wrapping key from a PIN/passphrase + salt using Argon2id.
pub fn derive_key(secret: &str, salt: &[u8]) -> Result<[u8; KEY_LEN]> {
    let params = Params::new(ARGON2_M_COST, ARGON2_T_COST, ARGON2_P_COST, Some(KEY_LEN))
        .map_err(|e| anyhow!("Argon2 params error: {e}"))?;
    let argon2 = Argon2::new(Algorithm::Argon2id, Version::V0x13, params);

    let mut derived = [0u8; KEY_LEN];
    argon2
        .hash_password_into(secret.as_bytes(), salt, &mut derived)
        .map_err(|e| anyhow!("Argon2 hash error: {e}"))?;

    Ok(derived)
}

/// Wrap (encrypt) the master key using a secret (PIN or passphrase).
/// Returns a WrappedKey containing the salt, nonce, and ciphertext.
pub fn wrap_key(master_key: &[u8; KEY_LEN], secret: &str) -> Result<WrappedKey> {
    // Random salt for Argon2
    let mut salt = [0u8; SALT_LEN];
    OsRng.fill_bytes(&mut salt);

    // Derive wrapping key from secret
    let mut wrapping_key = derive_key(secret, &salt)?;

    // Random nonce for AES-GCM
    let mut nonce_bytes = [0u8; NONCE_LEN];
    OsRng.fill_bytes(&mut nonce_bytes);

    // Encrypt master key
    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrapping_key));
    let nonce = Nonce::from_slice(&nonce_bytes);
    let ciphertext = cipher
        .encrypt(nonce, master_key.as_ref())
        .map_err(|e| anyhow!("AES-GCM encrypt error: {e}"))?;

    wrapping_key.zeroize();

    Ok(WrappedKey {
        salt: B64.encode(salt),
        nonce: B64.encode(nonce_bytes),
        ciphertext: B64.encode(ciphertext),
    })
}

/// Unwrap (decrypt) the master key using a secret (PIN or passphrase).
/// Returns the master key on success, or an error if the secret is wrong.
pub fn unwrap_key(wrapped: &WrappedKey, secret: &str) -> Result<[u8; KEY_LEN]> {
    let salt = B64.decode(&wrapped.salt)?;
    let nonce_bytes = B64.decode(&wrapped.nonce)?;
    let ciphertext = B64.decode(&wrapped.ciphertext)?;

    let mut wrapping_key = derive_key(secret, &salt)?;

    let cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&wrapping_key));
    let nonce = Nonce::from_slice(&nonce_bytes);

    let plaintext = cipher
        .decrypt(nonce, ciphertext.as_ref())
        .map_err(|_| anyhow!("Wrong PIN or passphrase"))?;

    wrapping_key.zeroize();

    if plaintext.len() != KEY_LEN {
        return Err(anyhow!("Unexpected key length after decryption"));
    }

    let mut master_key = [0u8; KEY_LEN];
    master_key.copy_from_slice(&plaintext);
    Ok(master_key)
}
