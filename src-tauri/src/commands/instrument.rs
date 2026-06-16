use crate::db;
use serde::{Deserialize, Serialize};

#[derive(Serialize)]
pub struct InstrumentSummary {
    pub instrument_id:         i64,
    pub isin:                  Option<String>,
    pub name:                  String,
    pub asset_class:           String,
    pub exchange_code:         Option<String>,
    pub nse_symbol:            Option<String>,
    pub amfi_code:             Option<String>,
    /// Set only when this result comes from `pending_instruments`.
    pub pending_instrument_id: Option<i64>,
    /// Raw metadata JSON from `pending_instruments.metadata` — used to pre-fill
    /// PendingInstrumentForm on the frontend.
    pub pending_metadata:      Option<String>,
}

#[derive(Deserialize)]
pub struct CreateInstrumentInput {
    pub isin: Option<String>,
    pub name: String,
    pub instrument_type_id: i64,
    pub primary_exchange_id: Option<i64>,
    pub nse_symbol: Option<String>,
    pub bse_code: Option<String>,
    pub amfi_code: Option<String>,
}

/// Search resolved instruments AND pending instruments by ISIN, symbol, AMFI code, or name.
/// Pending results carry a negative `instrument_id` (-pending_id) and populate
/// `pending_instrument_id` / `pending_metadata` so the frontend can pre-fill the
/// enrichment form before linking the transaction.
#[tauri::command]
pub fn search_instruments(query: String) -> Result<Vec<InstrumentSummary>, String> {
    let conn = db::acquire()?;
    let q      = query.trim().to_uppercase();
    let q_like = format!("%{}%", query.trim());

    let mut stmt = conn.prepare(
        "SELECT i.instrument_id,
                ie.isin,
                i.name,
                it.asset_class,
                e.code     AS exchange_code,
                ie.nse_symbol,
                im.amfi_code,
                NULL       AS pending_instrument_id,
                NULL       AS pending_metadata
         FROM instruments i
         JOIN instrument_types it ON i.instrument_type_id = it.instrument_type_id
         LEFT JOIN exchanges e ON i.primary_exchange_id = e.exchange_id
         LEFT JOIN instrument_equity ie ON ie.instrument_id = i.instrument_id
         LEFT JOIN instrument_mf     im ON im.instrument_id = i.instrument_id
         WHERE ie.isin       = ?1
            OR ie.nse_symbol LIKE ?2
            OR ie.bse_code   = ?1
            OR im.amfi_code  = ?1
            OR i.name        LIKE ?2

         UNION ALL

         SELECT -p.pending_id,
                NULL,
                p.name,
                p.type,
                NULL,
                NULL,
                NULL,
                p.pending_id,
                p.metadata
         FROM pending_instruments p
         WHERE p.name LIKE ?2

         ORDER BY name
         LIMIT 20"
    ).map_err(|e| e.to_string())?;

    let results = stmt.query_map(
        rusqlite::params![q, q_like],
        |row| {
            Ok(InstrumentSummary {
                instrument_id:         row.get(0)?,
                isin:                  row.get(1)?,
                name:                  row.get(2)?,
                asset_class:           row.get(3)?,
                exchange_code:         row.get(4)?,
                nse_symbol:            row.get(5)?,
                amfi_code:             row.get(6)?,
                pending_instrument_id: row.get(7)?,
                pending_metadata:      row.get(8)?,
            })
        },
    )
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    Ok(results)
}

/// Get all instrument types (for manual instrument creation).
#[tauri::command]
pub fn get_instrument_types() -> Result<Vec<(i64, String, String)>, String> {
    let conn = db::acquire()?;
    let mut stmt = conn.prepare(
        "SELECT instrument_type_id, name, asset_class FROM instrument_types ORDER BY name"
    ).map_err(|e| e.to_string())?;

    let types = stmt.query_map([], |row| {
        Ok((row.get::<_, i64>(0)?, row.get::<_, String>(1)?, row.get::<_, String>(2)?))
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    Ok(types)
}

/// Create a new instrument manually (when not found in local DB or server).
/// MANUAL instruments receive a locally-assigned instrument_id (SQLite rowid
/// auto-assign) since they have no server counterpart. They are excluded from
/// server resolution calls (source = 'MANUAL').
#[tauri::command]
pub fn create_instrument(input: CreateInstrumentInput) -> Result<InstrumentSummary, String> {
    let conn = db::acquire()?;

    conn.execute(
        "INSERT INTO instruments (name, instrument_type_id, primary_exchange_id, source)
         VALUES (?1, ?2, ?3, 'MANUAL')",
        rusqlite::params![
            input.name,
            input.instrument_type_id,
            input.primary_exchange_id,
        ],
    ).map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();

    // Insert equity extension if any equity field is present (isin, nse_symbol, bse_code)
    if input.isin.is_some() || input.nse_symbol.is_some() || input.bse_code.is_some() {
        conn.execute(
            "INSERT INTO instrument_equity (instrument_id, isin, nse_symbol, bse_code)
             VALUES (?1, ?2, ?3, ?4)",
            rusqlite::params![id, input.isin, input.nse_symbol, input.bse_code],
        ).map_err(|e| e.to_string())?;
    }

    // Insert MF extension if amfi_code provided
    if let Some(ref amfi) = input.amfi_code {
        conn.execute(
            "INSERT INTO instrument_mf (instrument_id, amfi_code) VALUES (?1, ?2)",
            rusqlite::params![id, amfi],
        ).map_err(|e| e.to_string())?;
    }

    Ok(InstrumentSummary {
        instrument_id:         id,
        isin:                  input.isin,
        name:                  input.name,
        asset_class:           String::new(),
        exchange_code:         None,
        nse_symbol:            input.nse_symbol,
        amfi_code:             input.amfi_code,
        pending_instrument_id: None,
        pending_metadata:      None,
    })
}

/// Update the name and metadata of an existing pending instrument.
/// Called when a user enriches a pending instrument (adds ISIN / symbol)
/// before or after linking it to a transaction.
#[tauri::command]
pub fn update_pending_instrument(
    pending_id: i64,
    name:       String,
    metadata:   serde_json::Value,
) -> Result<(), String> {
    let conn          = db::acquire()?;
    let metadata_json = serde_json::to_string(&metadata).map_err(|e| e.to_string())?;
    conn.execute(
        "UPDATE pending_instruments SET name = ?1, metadata = ?2 WHERE pending_id = ?3",
        rusqlite::params![name, metadata_json, pending_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}
