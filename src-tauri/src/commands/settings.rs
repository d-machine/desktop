use crate::db;

#[tauri::command]
pub fn get_setting(key: String) -> Result<Option<String>, String> {
    let conn = db::acquire()?;
    let result = conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        [&key],
        |row| row.get::<_, String>(0),
    );

    match result {
        Ok(val) => Ok(Some(val)),
        Err(rusqlite::Error::QueryReturnedNoRows) => Ok(None),
        Err(e) => Err(e.to_string()),
    }
}

#[tauri::command]
pub fn set_setting(key: String, value: String) -> Result<(), String> {
    let conn = db::acquire()?;
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at)
         VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at",
        [&key, &value],
    )
    .map_err(|e| e.to_string())?;
    Ok(())
}
