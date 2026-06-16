use crate::db;
use serde::{Deserialize, Serialize};

pub const CHARGE_TYPES: &[&str] = &[
    "BROKERAGE", "STT", "GST", "EXCHANGE", "SEBI", "STAMP_DUTY", "DP", "OTHER",
];

#[derive(Serialize, Clone)]
pub struct Charge {
    pub charge_id: i64,
    pub account_id: i64,
    pub account_name: String,
    pub start_date: String,
    pub end_date: String,
    pub charge_type: String,
    pub amount_paise: i64,
    pub source: String,
    pub import_batch_id: Option<i64>,
    pub notes: Option<String>,
    pub created_at: String,
}

#[derive(Deserialize)]
pub struct ChargeInput {
    pub account_id: i64,
    pub start_date: String,
    pub end_date: String,
    pub charge_type: String,
    pub amount_paise: i64,
    pub notes: Option<String>,
}

#[tauri::command]
pub fn get_charges(account_ids: Option<Vec<i64>>) -> Result<Vec<Charge>, String> {
    let conn = db::acquire()?;

    let acct_sql = match &account_ids {
        Some(ids) if !ids.is_empty() => {
            let ph = ids.iter().enumerate()
                .map(|(i, _)| format!("?{}", i + 1))
                .collect::<Vec<_>>().join(",");
            format!("WHERE c.account_id IN ({})", ph)
        }
        _ => String::new(),
    };

    let sql = format!(
        "SELECT c.charge_id, c.account_id, a.name,
                c.start_date, c.end_date, c.charge_type,
                c.amount_paise, c.source, c.import_batch_id,
                c.notes, c.created_at
         FROM charges c
         JOIN accounts a ON a.account_id = c.account_id
         {acct_sql}
         ORDER BY c.end_date DESC, c.start_date DESC, c.charge_id DESC"
    );

    let acct_params: Vec<Box<dyn rusqlite::ToSql>> = match &account_ids {
        Some(ids) if !ids.is_empty() => {
            ids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>).collect()
        }
        _ => vec![],
    };

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(acct_params.iter().map(|p| p.as_ref())),
        |row| Ok(Charge {
            charge_id:       row.get(0)?,
            account_id:      row.get(1)?,
            account_name:    row.get(2)?,
            start_date:      row.get(3)?,
            end_date:        row.get(4)?,
            charge_type:     row.get(5)?,
            amount_paise:    row.get(6)?,
            source:          row.get(7)?,
            import_batch_id: row.get(8)?,
            notes:           row.get(9)?,
            created_at:      row.get(10)?,
        }),
    )
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    Ok(rows)
}

#[tauri::command]
pub fn create_charge(input: ChargeInput) -> Result<Charge, String> {
    validate_charge(&input)?;
    let conn = db::acquire()?;

    conn.execute(
        "INSERT INTO charges (account_id, start_date, end_date, charge_type, amount_paise, source, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, 'MANUAL', ?6)",
        rusqlite::params![
            input.account_id, input.start_date, input.end_date,
            input.charge_type, input.amount_paise, input.notes,
        ],
    ).map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    fetch_one(&conn, id)
}

#[tauri::command]
pub fn update_charge(charge_id: i64, input: ChargeInput) -> Result<Charge, String> {
    validate_charge(&input)?;
    let conn = db::acquire()?;

    let rows = conn.execute(
        "UPDATE charges
         SET account_id = ?1, start_date = ?2, end_date = ?3,
             charge_type = ?4, amount_paise = ?5, notes = ?6
         WHERE charge_id = ?7",
        rusqlite::params![
            input.account_id, input.start_date, input.end_date,
            input.charge_type, input.amount_paise, input.notes, charge_id,
        ],
    ).map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err(format!("Charge {} not found", charge_id));
    }
    fetch_one(&conn, charge_id)
}

#[tauri::command]
pub fn delete_charge(charge_id: i64) -> Result<(), String> {
    let conn = db::acquire()?;
    let rows = conn.execute(
        "DELETE FROM charges WHERE charge_id = ?1",
        rusqlite::params![charge_id],
    ).map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err(format!("Charge {} not found", charge_id));
    }
    Ok(())
}

fn validate_charge(input: &ChargeInput) -> Result<(), String> {
    if !CHARGE_TYPES.contains(&input.charge_type.as_str()) {
        return Err(format!("Invalid charge type: {}", input.charge_type));
    }
    if input.start_date > input.end_date {
        return Err("Start date must be ≤ end date".to_string());
    }
    if input.amount_paise <= 0 {
        return Err("Amount must be greater than zero".to_string());
    }
    Ok(())
}

fn fetch_one(conn: &rusqlite::Connection, charge_id: i64) -> Result<Charge, String> {
    conn.query_row(
        "SELECT c.charge_id, c.account_id, a.name,
                c.start_date, c.end_date, c.charge_type,
                c.amount_paise, c.source, c.import_batch_id,
                c.notes, c.created_at
         FROM charges c
         JOIN accounts a ON a.account_id = c.account_id
         WHERE c.charge_id = ?1",
        rusqlite::params![charge_id],
        |row| Ok(Charge {
            charge_id:       row.get(0)?,
            account_id:      row.get(1)?,
            account_name:    row.get(2)?,
            start_date:      row.get(3)?,
            end_date:        row.get(4)?,
            charge_type:     row.get(5)?,
            amount_paise:    row.get(6)?,
            source:          row.get(7)?,
            import_batch_id: row.get(8)?,
            notes:           row.get(9)?,
            created_at:      row.get(10)?,
        }),
    ).map_err(|e| e.to_string())
}
