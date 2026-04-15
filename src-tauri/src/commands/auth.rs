use std::path::PathBuf;

use anyhow::Result;
use serde::{Deserialize, Serialize};
use tauri::AppHandle;
use tauri::Manager;

use crate::auth::crypto::{
    generate_master_key, wrap_key, unwrap_key,
    AuthConfig, RecoveryFile,
};
use crate::auth::state;
use crate::db;

fn auth_config_path(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap().join("auth.json")
}

fn db_path(app: &AppHandle) -> PathBuf {
    app.path().app_data_dir().unwrap().join("portfolio.db")
}

/// Returns true if the app has been set up (auth.json exists).
#[tauri::command]
pub fn is_setup(app: AppHandle) -> bool {
    auth_config_path(&app).exists()
}

/// Returns true if the app is currently unlocked.
#[tauri::command]
pub fn is_unlocked() -> bool {
    state::get().lock().unwrap().unlocked
}

/// First-time setup: set PIN + recovery passphrase.
/// Generates master key, wraps it twice, saves auth.json, returns recovery file JSON.
#[tauri::command]
pub fn setup(app: AppHandle, pin: String, passphrase: String) -> Result<String, String> {
    let config_path = auth_config_path(&app);
    if config_path.exists() {
        return Err("App is already set up".into());
    }

    // Generate master key
    let master_key = generate_master_key();

    // Wrap with PIN → auth.json
    let pin_wrapped = wrap_key(&master_key, &pin).map_err(|e| e.to_string())?;
    let config = AuthConfig { version: 1, pin_wrapped };
    let config_json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::create_dir_all(config_path.parent().unwrap()).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, &config_json).map_err(|e| e.to_string())?;

    // Wrap with passphrase → recovery file (returned to frontend for saving)
    let passphrase_wrapped = wrap_key(&master_key, &passphrase).map_err(|e| e.to_string())?;
    let recovery = RecoveryFile { version: 1, passphrase_wrapped };
    let recovery_json = serde_json::to_string_pretty(&recovery).map_err(|e| e.to_string())?;

    // Unlock the app immediately after setup
    db::init(db_path(&app)).map_err(|e| e.to_string())?;
    state::get().lock().unwrap().unlock(master_key);

    Ok(recovery_json) // frontend saves this as .ptbak file via dialog
}

/// Normal login: verify PIN and unlock the app.
#[tauri::command]
pub fn login(app: AppHandle, pin: String) -> Result<(), String> {
    let config_path = auth_config_path(&app);
    let config_json = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let config: AuthConfig = serde_json::from_str(&config_json).map_err(|e| e.to_string())?;

    let master_key = unwrap_key(&config.pin_wrapped, &pin).map_err(|_| "Wrong PIN".to_string())?;

    db::init(db_path(&app)).map_err(|e| e.to_string())?;
    state::get().lock().unwrap().unlock(master_key);

    Ok(())
}

/// Lock the app — wipes master key from memory.
#[tauri::command]
pub fn lock() {
    state::get().lock().unwrap().lock();
}

/// Forgot PIN: recover using recovery file + passphrase, then set a new PIN.
#[tauri::command]
pub fn recover(
    app: AppHandle,
    recovery_file_contents: String,
    passphrase: String,
    new_pin: String,
) -> Result<(), String> {
    // Unwrap master key using passphrase
    let recovery: RecoveryFile =
        serde_json::from_str(&recovery_file_contents).map_err(|e| e.to_string())?;
    let master_key = unwrap_key(&recovery.passphrase_wrapped, &passphrase)
        .map_err(|_| "Wrong recovery passphrase".to_string())?;

    // Re-wrap with new PIN and save
    let pin_wrapped = wrap_key(&master_key, &new_pin).map_err(|e| e.to_string())?;
    let config = AuthConfig { version: 1, pin_wrapped };
    let config_json = serde_json::to_string_pretty(&config).map_err(|e| e.to_string())?;
    std::fs::write(auth_config_path(&app), &config_json).map_err(|e| e.to_string())?;

    // Unlock
    db::init(db_path(&app)).map_err(|e| e.to_string())?;
    state::get().lock().unwrap().unlock(master_key);

    Ok(())
}

/// Change PIN: requires current PIN to be correct.
#[tauri::command]
pub fn change_pin(app: AppHandle, current_pin: String, new_pin: String) -> Result<(), String> {
    let config_path = auth_config_path(&app);
    let config_json = std::fs::read_to_string(&config_path).map_err(|e| e.to_string())?;
    let config: AuthConfig = serde_json::from_str(&config_json).map_err(|e| e.to_string())?;

    // Verify current PIN
    let master_key = unwrap_key(&config.pin_wrapped, &current_pin)
        .map_err(|_| "Current PIN is incorrect".to_string())?;

    // Re-wrap with new PIN
    let pin_wrapped = wrap_key(&master_key, &new_pin).map_err(|e| e.to_string())?;
    let new_config = AuthConfig { version: 1, pin_wrapped };
    let new_json = serde_json::to_string_pretty(&new_config).map_err(|e| e.to_string())?;
    std::fs::write(&config_path, new_json).map_err(|e| e.to_string())?;

    Ok(())
}
