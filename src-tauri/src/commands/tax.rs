use crate::db;
use serde::{Deserialize, Serialize};

pub const ENTRY_TYPES: &[&str] = &["TDS", "ADVANCE_TAX", "SELF_ASSESSMENT_TAX"];

#[derive(Serialize, Clone)]
pub struct TaxEntry {
    pub entry_id:     i64,
    pub person_id:    i64,
    pub person_name:  String,
    pub entry_type:   String,
    pub amount_paise: i64,
    pub entry_date:   String,
    pub fy:           String,
    pub txn_id:       Option<i64>,
    pub notes:        Option<String>,
    pub created_at:   String,
}

#[derive(Deserialize)]
pub struct TaxEntryInput {
    pub person_id:    i64,
    pub entry_type:   String,
    pub amount_paise: i64,
    pub entry_date:   String,
    pub fy:           String,
    pub txn_id:       Option<i64>,
    pub notes:        Option<String>,
}

#[tauri::command]
pub fn get_tax_entries(
    person_id: Option<i64>,
    fy:        Option<String>,
) -> Result<Vec<TaxEntry>, String> {
    let conn = db::acquire()?;

    let mut sql = String::from(
        "SELECT e.entry_id, e.person_id, p.name,
                e.entry_type, e.amount_paise, e.entry_date, e.fy,
                e.txn_id, e.notes, e.created_at
         FROM tax_entries e
         JOIN persons p ON p.person_id = e.person_id
         WHERE 1=1",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];

    if let Some(pid) = person_id {
        sql.push_str(" AND e.person_id = ?");
        params.push(Box::new(pid));
    }
    if let Some(ref f) = fy {
        sql.push_str(" AND e.fy = ?");
        params.push(Box::new(f.clone()));
    }
    sql.push_str(" ORDER BY e.entry_date DESC, e.entry_id DESC");

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows = stmt.query_map(
        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
        |row| Ok(TaxEntry {
            entry_id:     row.get(0)?,
            person_id:    row.get(1)?,
            person_name:  row.get(2)?,
            entry_type:   row.get(3)?,
            amount_paise: row.get(4)?,
            entry_date:   row.get(5)?,
            fy:           row.get(6)?,
            txn_id:       row.get(7)?,
            notes:        row.get(8)?,
            created_at:   row.get(9)?,
        }),
    )
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    Ok(rows)
}

#[tauri::command]
pub fn create_tax_entry(input: TaxEntryInput) -> Result<TaxEntry, String> {
    validate(&input)?;
    let conn = db::acquire()?;

    conn.execute(
        "INSERT INTO tax_entries
            (person_id, entry_type, amount_paise, entry_date, fy, txn_id, notes)
         VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7)",
        rusqlite::params![
            input.person_id, input.entry_type, input.amount_paise,
            input.entry_date, input.fy, input.txn_id, input.notes,
        ],
    ).map_err(|e| e.to_string())?;

    fetch_one(&conn, conn.last_insert_rowid())
}

#[tauri::command]
pub fn delete_tax_entry(entry_id: i64) -> Result<(), String> {
    let conn = db::acquire()?;
    let rows = conn.execute(
        "DELETE FROM tax_entries WHERE entry_id = ?1",
        rusqlite::params![entry_id],
    ).map_err(|e| e.to_string())?;

    if rows == 0 {
        return Err(format!("Tax entry {} not found", entry_id));
    }
    Ok(())
}

fn validate(input: &TaxEntryInput) -> Result<(), String> {
    if !ENTRY_TYPES.contains(&input.entry_type.as_str()) {
        return Err(format!("Invalid entry type: {}", input.entry_type));
    }
    if input.amount_paise <= 0 {
        return Err("Amount must be greater than zero".to_string());
    }
    if input.entry_date.is_empty() {
        return Err("Entry date is required".to_string());
    }
    if input.fy.is_empty() {
        return Err("Financial year is required".to_string());
    }
    Ok(())
}

fn fetch_one(conn: &rusqlite::Connection, entry_id: i64) -> Result<TaxEntry, String> {
    conn.query_row(
        "SELECT e.entry_id, e.person_id, p.name,
                e.entry_type, e.amount_paise, e.entry_date, e.fy,
                e.txn_id, e.notes, e.created_at
         FROM tax_entries e
         JOIN persons p ON p.person_id = e.person_id
         WHERE e.entry_id = ?1",
        rusqlite::params![entry_id],
        |row| Ok(TaxEntry {
            entry_id:     row.get(0)?,
            person_id:    row.get(1)?,
            person_name:  row.get(2)?,
            entry_type:   row.get(3)?,
            amount_paise: row.get(4)?,
            entry_date:   row.get(5)?,
            fy:           row.get(6)?,
            txn_id:       row.get(7)?,
            notes:        row.get(8)?,
            created_at:   row.get(9)?,
        }),
    ).map_err(|e| e.to_string())
}
