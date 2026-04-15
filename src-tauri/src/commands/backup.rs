use tauri::Manager;
use tauri_plugin_dialog::DialogExt;
use crate::auth::{crypto, state as auth_state};
use crate::db;
use aes_gcm::{
    aead::{Aead, KeyInit},
    Aes256Gcm, Key, Nonce,
};
use rand::{rngs::OsRng, RngCore};
use zeroize::Zeroize;
use std::io::{Write as _};
use std::path::PathBuf;

// .ptdata file magic + version
const MAGIC: &[u8; 4] = b"PTDT";
const VERSION: u32 = 2;  // v2: plain SQLite (no SQLCipher layer)

#[tauri::command]
pub async fn pick_backup_folder(app: tauri::AppHandle) -> Result<Option<String>, String> {
    let folder = app
        .dialog()
        .file()
        .set_title("Select Backup Folder")
        .blocking_pick_folder();
    match folder {
        Some(path) => Ok(Some(path.to_string())),
        None => Ok(None),
    }
}

/// Export the database to a .ptdata file encrypted with the given password.
#[tauri::command]
pub fn export_data(
    app: tauri::AppHandle,
    pin: String,
    password: String,
    dest_path: String,
) -> Result<(), String> {
    // Verify PIN (security gate — user must prove identity before exporting)
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;
    let auth_path = app_dir.join("auth.json");
    let auth_json = std::fs::read_to_string(&auth_path).map_err(|e| e.to_string())?;
    let auth_config: crypto::AuthConfig = serde_json::from_str(&auth_json).map_err(|e| e.to_string())?;
    crypto::unwrap_key(&auth_config.pin_wrapped, &pin)
        .map_err(|_| "Incorrect PIN".to_string())?;

    // Read the plain SQLite file directly
    let db_path = app_dir.join("portfolio.db");
    let plaintext_bytes = std::fs::read(&db_path).map_err(|e| e.to_string())?;

    // Encrypt with a fresh random export key (AES-256-GCM)
    let mut export_key = [0u8; 32];
    OsRng.fill_bytes(&mut export_key);

    let mut db_nonce_bytes = [0u8; 12];
    OsRng.fill_bytes(&mut db_nonce_bytes);

    let db_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&export_key));
    let db_nonce = Nonce::from_slice(&db_nonce_bytes);
    let encrypted_db = db_cipher
        .encrypt(db_nonce, plaintext_bytes.as_ref())
        .map_err(|e| format!("DB encryption failed: {e}"))?;

    // Wrap export key with export password (Argon2id)
    let wrapped_export_key = crypto::wrap_key(&export_key, &password)
        .map_err(|e| e.to_string())?;
    export_key.zeroize();

    // Write .ptdata file
    let salt_bytes = base64_decode(&wrapped_export_key.salt)?;
    let wrap_nonce_bytes = base64_decode(&wrapped_export_key.nonce)?;
    let wrap_ct_bytes = base64_decode(&wrapped_export_key.ciphertext)?;

    let mut file = std::fs::File::create(&dest_path).map_err(|e| e.to_string())?;
    file.write_all(MAGIC).map_err(|e| e.to_string())?;
    file.write_all(&VERSION.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(&salt_bytes).map_err(|e| e.to_string())?;
    file.write_all(&wrap_nonce_bytes).map_err(|e| e.to_string())?;
    let wct_len = wrap_ct_bytes.len() as u32;
    file.write_all(&wct_len.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(&wrap_ct_bytes).map_err(|e| e.to_string())?;
    file.write_all(&db_nonce_bytes).map_err(|e| e.to_string())?;
    let db_len = encrypted_db.len() as u64;
    file.write_all(&db_len.to_le_bytes()).map_err(|e| e.to_string())?;
    file.write_all(&encrypted_db).map_err(|e| e.to_string())?;
    file.flush().map_err(|e| e.to_string())?;

    Ok(())
}

/// Import a .ptdata file — wipes current DB and replaces with the imported one.
#[tauri::command]
pub fn import_data(
    app: tauri::AppHandle,
    password: String,
    src_path: String,
) -> Result<(), String> {
    let app_dir = app.path().app_data_dir().map_err(|e| e.to_string())?;

    // Read .ptdata file
    let file_bytes = std::fs::read(&src_path).map_err(|e| e.to_string())?;
    let mut cursor = 0usize;

    if file_bytes.get(cursor..cursor + 4) != Some(MAGIC) {
        return Err("Invalid file format — not a .ptdata file".to_string());
    }
    cursor += 4;

    let version = u32::from_le_bytes(
        file_bytes[cursor..cursor + 4].try_into().map_err(|_| "Corrupt file")?
    );
    cursor += 4;
    if version != VERSION {
        return Err(format!("Unsupported .ptdata version {version} (expected {VERSION})"));
    }

    let salt_bytes: [u8; 16] = file_bytes[cursor..cursor + 16]
        .try_into().map_err(|_| "Corrupt file: salt")?;
    cursor += 16;

    let wrap_nonce_bytes: [u8; 12] = file_bytes[cursor..cursor + 12]
        .try_into().map_err(|_| "Corrupt file: wrap nonce")?;
    cursor += 12;

    let wct_len = u32::from_le_bytes(
        file_bytes[cursor..cursor + 4].try_into().map_err(|_| "Corrupt file: wct len")?
    ) as usize;
    cursor += 4;
    let wrap_ct_bytes = file_bytes[cursor..cursor + wct_len].to_vec();
    cursor += wct_len;

    let db_nonce_bytes: [u8; 12] = file_bytes[cursor..cursor + 12]
        .try_into().map_err(|_| "Corrupt file: db nonce")?;
    cursor += 12;

    let db_len = u64::from_le_bytes(
        file_bytes[cursor..cursor + 8].try_into().map_err(|_| "Corrupt file: db len")?
    ) as usize;
    cursor += 8;
    let encrypted_db = &file_bytes[cursor..cursor + db_len];

    // Unwrap export key using password
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    let wrapped = crypto::WrappedKey {
        salt: B64.encode(salt_bytes),
        nonce: B64.encode(wrap_nonce_bytes),
        ciphertext: B64.encode(&wrap_ct_bytes),
    };
    let mut export_key = crypto::unwrap_key(&wrapped, &password)
        .map_err(|_| "Incorrect export password".to_string())?;

    // Decrypt the db blob
    let db_cipher = Aes256Gcm::new(Key::<Aes256Gcm>::from_slice(&export_key));
    let db_nonce = Nonce::from_slice(&db_nonce_bytes);
    let plaintext_bytes = db_cipher
        .decrypt(db_nonce, encrypted_db)
        .map_err(|_| "Decryption failed — file may be corrupt or wrong password".to_string())?;
    export_key.zeroize();

    // Write plaintext to temp file then atomically replace the DB
    let db_path = app_dir.join("portfolio.db");
    let temp_path = app_dir.join("import_temp.db");

    std::fs::write(&temp_path, &plaintext_bytes).map_err(|e| {
        format!("Failed to write temp file: {e}")
    })?;

    // Remove stale WAL/SHM files
    let _ = std::fs::remove_file(app_dir.join("portfolio.db-wal"));
    let _ = std::fs::remove_file(app_dir.join("portfolio.db-shm"));

    std::fs::rename(&temp_path, &db_path).map_err(|e| {
        let _ = std::fs::remove_file(&temp_path);
        format!("Failed to replace database: {e}")
    })?;

    db::replace(&db_path)?;

    Ok(())
}

fn base64_decode(s: &str) -> Result<Vec<u8>, String> {
    use base64::{engine::general_purpose::STANDARD as B64, Engine};
    B64.decode(s).map_err(|e| format!("base64 decode error: {e}"))
}
