//! Auth and key management.
//!
//! Design:
//!   - A random 32-byte master key encrypts the SQLite DB (via SQLCipher).
//!   - The master key is never stored in plaintext.
//!   - It is stored twice, each time wrapped (AES-256-GCM) with a different derived key:
//!       Copy 1 — wrapped with PIN-derived key     → stored in `auth.json` in app data dir
//!       Copy 2 — wrapped with passphrase-derived key → written to `.ptbak` recovery file
//!   - Key derivation uses Argon2id (memory-hard, brute-force resistant).
//!
//! Recovery flow:
//!   Forgot PIN → load recovery file → enter passphrase → unwrap master key →
//!   set new PIN → re-wrap master key with new PIN → overwrite Copy 1.

pub mod crypto;
pub mod state;
