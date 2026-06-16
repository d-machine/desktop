//! Shared utilities for all import pipelines.

use crate::db;
use tauri::Manager;

// ─── Instrument resolution ────────────────────────────────────────────────────

/// Try to resolve an equity instrument to a known `instrument_id`, or stage it
/// as a pending instrument. Returns `Some((instrument_id, pending_instrument_id))`
/// where exactly one of the two is `Some`.
///
/// Resolution order:
///   1. ISIN  → instrument_equity.isin
///   2. BSE code → instrument_equity.bse_code
///   3. NSE symbol → instrument_equity.nse_symbol
///   4. Existing pending_instrument with same ISIN or name
///   5. New pending_instrument row (JSON metadata)
pub fn resolve_equity(
    conn:         &rusqlite::Connection,
    name:         &str,
    isin:         Option<&str>,
    bse_code:     Option<&str>,
    nse_symbol:   Option<&str>,
    exchange:     Option<&str>,
    auto_created: &mut usize,
) -> Option<(Option<i64>, Option<i64>)> {
    // 1. Resolve by ISIN
    if let Some(isin) = isin.filter(|s| !s.is_empty()) {
        if let Ok(id) = conn.query_row(
            "SELECT instrument_id FROM instrument_equity WHERE isin=?1 LIMIT 1",
            [isin], |r| r.get::<_, i64>(0),
        ) {
            return Some((Some(id), None));
        }
    }

    // 2. Resolve by BSE code
    if let Some(bse) = bse_code.filter(|s| !s.is_empty()) {
        if let Ok(id) = conn.query_row(
            "SELECT instrument_id FROM instrument_equity WHERE bse_code=?1 LIMIT 1",
            [bse], |r| r.get::<_, i64>(0),
        ) {
            return Some((Some(id), None));
        }
    }

    // 3. Resolve by NSE symbol
    if let Some(sym) = nse_symbol.filter(|s| !s.is_empty()) {
        if let Ok(id) = conn.query_row(
            "SELECT instrument_id FROM instrument_equity WHERE nse_symbol=?1 LIMIT 1",
            [sym], |r| r.get::<_, i64>(0),
        ) {
            return Some((Some(id), None));
        }
    }

    // 4. Look for an existing pending_instrument (avoid duplicates within a session)
    if let Some(isin) = isin.filter(|s| !s.is_empty()) {
        if let Ok(id) = conn.query_row(
            "SELECT pending_id FROM pending_instruments
             WHERE type='EQUITY' AND json_extract(metadata,'$.isin')=?1 LIMIT 1",
            [isin], |r| r.get::<_, i64>(0),
        ) {
            return Some((None, Some(id)));
        }
    } else {
        if let Ok(id) = conn.query_row(
            "SELECT pending_id FROM pending_instruments
             WHERE type='EQUITY' AND name=?1 LIMIT 1",
            [name], |r| r.get::<_, i64>(0),
        ) {
            return Some((None, Some(id)));
        }
    }

    // 5. Create new pending_instrument with JSON metadata
    let metadata = serde_json::json!({
        "isin":       isin.filter(|s| !s.is_empty()),
        "bse_code":   bse_code.filter(|s| !s.is_empty()),
        "nse_symbol": nse_symbol.filter(|s| !s.is_empty()),
        "exchange":   exchange.filter(|s| !s.is_empty()),
    });

    match conn.execute(
        "INSERT INTO pending_instruments (name, type, metadata) VALUES (?1, 'EQUITY', ?2)",
        rusqlite::params![name, metadata.to_string()],
    ) {
        Ok(_) => {
            *auto_created += 1;
            Some((None, Some(conn.last_insert_rowid())))
        }
        Err(_) => None,
    }
}

// ─── File copy ────────────────────────────────────────────────────────────────

/// Copy source PDFs into the app's `statements/` data directory, prepending a
/// timestamp to avoid filename collisions.  Returns a JSON array string of the
/// final stored paths (suitable for `import_batches.file_name`), or `None` if
/// no valid paths were processed.
pub fn copy_statements(app: &tauri::AppHandle, file_paths: Vec<String>) -> Option<String> {
    let app_data = app.path().app_data_dir().ok()?;
    let stmts_dir = app_data.join("statements");
    let _ = std::fs::create_dir_all(&stmts_dir);

    let ts = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap_or_default()
        .as_secs();

    let mut stored: Vec<String> = Vec::new();
    for src in file_paths {
        let p = std::path::Path::new(&src);
        let fname = p.file_name().unwrap_or_default();
        let dest_name = format!("{}_{}", ts, fname.to_string_lossy());
        let dest = stmts_dir.join(&dest_name);
        if p.exists() && std::fs::copy(p, &dest).is_ok() {
            stored.push(dest.to_string_lossy().into_owned());
        } else {
            stored.push(src);
        }
    }

    if stored.is_empty() {
        None
    } else {
        serde_json::to_string(&stored).ok()
    }
}

// ─── Batch file name update ───────────────────────────────────────────────────

/// Set `file_name` on a list of import_batches rows.
pub fn update_batch_file_names(
    conn:           &rusqlite::Connection,
    batch_ids:      &[i64],
    file_name_json: &str,
) {
    for &bid in batch_ids {
        let _ = conn.execute(
            "UPDATE import_batches SET file_name=?1 WHERE batch_id=?2",
            rusqlite::params![file_name_json, bid],
        );
    }
}

// ─── Derivative instrument resolution ────────────────────────────────────────

/// Resolve a futures/options instrument to a known `instrument_id`, or stage it
/// as a pending instrument. Mirrors the server's `_resolve_fo` logic.
///
/// Resolution order:
///   1. `instrument_derivatives` by (underlying_symbol, expiry_date, instrument_type) [+ strike]
///   2. Existing `pending_instruments` with same (type, underlying_symbol, expiry_date) [+ strike]
///   3. New `pending_instruments` row
pub fn resolve_derivative(
    conn:            &rusqlite::Connection,
    name:            &str,
    underlying:      &str,
    expiry_date:     &str,
    pending_type:    &str,   // FUTSTK | FUTIDX | OPTSTK | OPTIDX
    instrument_type: &str,   // FUTURES | OPTIONS
    exchange:        &str,
    strike_paise:    Option<i64>,
    option_type:     Option<&str>,
    auto_created:    &mut usize,
) -> Option<(Option<i64>, Option<i64>)> {
    let sym_up = underlying.to_uppercase();

    // 1. Look up instrument_derivatives
    let instrument_id: Option<i64> = if let Some(strike) = strike_paise {
        conn.query_row(
            "SELECT d.instrument_id FROM instrument_derivatives d
             WHERE UPPER(d.underlying_symbol) = ?1
               AND d.expiry_date              = ?2
               AND d.instrument_type          = ?3
               AND d.strike_price_paise       = ?4
             LIMIT 1",
            rusqlite::params![sym_up, expiry_date, instrument_type, strike],
            |r| r.get(0),
        ).ok()
    } else {
        conn.query_row(
            "SELECT d.instrument_id FROM instrument_derivatives d
             WHERE UPPER(d.underlying_symbol) = ?1
               AND d.expiry_date              = ?2
               AND d.instrument_type          = ?3
             LIMIT 1",
            rusqlite::params![sym_up, expiry_date, instrument_type],
            |r| r.get(0),
        ).ok()
    };

    if let Some(id) = instrument_id {
        return Some((Some(id), None));
    }

    // 2. Look up pending_instruments
    let pending_id: Option<i64> = if let Some(strike) = strike_paise {
        conn.query_row(
            "SELECT pending_id FROM pending_instruments
             WHERE type = ?1
               AND UPPER(json_extract(metadata, '$.underlying_symbol')) = ?2
               AND json_extract(metadata, '$.expiry_date') = ?3
               AND CAST(json_extract(metadata, '$.strike_price_paise') AS INTEGER) = ?4
             LIMIT 1",
            rusqlite::params![pending_type, sym_up, expiry_date, strike],
            |r| r.get(0),
        ).ok()
    } else {
        conn.query_row(
            "SELECT pending_id FROM pending_instruments
             WHERE type = ?1
               AND UPPER(json_extract(metadata, '$.underlying_symbol')) = ?2
               AND json_extract(metadata, '$.expiry_date') = ?3
             LIMIT 1",
            rusqlite::params![pending_type, sym_up, expiry_date],
            |r| r.get(0),
        ).ok()
    };

    if let Some(id) = pending_id {
        return Some((None, Some(id)));
    }

    // 3. Create new pending_instrument
    let metadata = if let Some(strike) = strike_paise {
        serde_json::json!({
            "underlying_symbol": underlying,
            "expiry_date":        expiry_date,
            "exchange":           exchange,
            "strike_price_paise": strike,
            "option_type":        option_type,
        })
    } else {
        serde_json::json!({
            "underlying_symbol": underlying,
            "expiry_date":        expiry_date,
            "exchange":           exchange,
        })
    };

    match conn.execute(
        "INSERT INTO pending_instruments (name, type, metadata) VALUES (?1, ?2, ?3)",
        rusqlite::params![name, pending_type, metadata.to_string()],
    ) {
        Ok(_) => {
            *auto_created += 1;
            Some((None, Some(conn.last_insert_rowid())))
        }
        Err(_) => None,
    }
}

// ─── Create pending instrument (used by manual transaction entry) ─────────────

#[derive(serde::Deserialize)]
pub struct PendingInstrumentSpec {
    pub name:     String,
    #[serde(rename = "type")]
    pub kind:     String,   // EQUITY | MF | FUTSTK | FUTIDX | OPTSTK | OPTIDX | MCX
    pub metadata: serde_json::Value,
}

/// Insert a new pending_instruments row from a user-supplied spec and return
/// its `pending_id`.  Existing rows are NOT deduplicated here — callers that
/// want dedup should query first.
pub fn create_pending_instrument(
    spec: &PendingInstrumentSpec,
) -> Result<i64, String> {
    let conn = db::acquire()?;
    conn.execute(
        "INSERT INTO pending_instruments (name, type, metadata) VALUES (?1, ?2, ?3)",
        rusqlite::params![spec.name, spec.kind, spec.metadata.to_string()],
    ).map_err(|e| e.to_string())?;
    Ok(conn.last_insert_rowid())
}
