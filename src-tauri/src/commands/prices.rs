use crate::db;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

// ── Public result types ───────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SyncPricesResult {
    pub updated:   usize,
    pub synced_at: String,
}

#[derive(Serialize)]
pub struct ResolveResult {
    pub resolved:  usize,   // instruments successfully matched to server
    pub merged:    usize,   // duplicate pairs merged into one
    pub unmatched: usize,   // instruments sent but not found on server
}

// ── Server request / response shapes ─────────────────────────────────────────

#[derive(Serialize)]
struct InstrumentRef {
    client_instrument_id: i64,
    #[serde(skip_serializing_if = "Option::is_none")]
    isin:       Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    nse_symbol: Option<String>,
    #[serde(skip_serializing_if = "Option::is_none")]
    bse_code:   Option<String>,
}

#[derive(Deserialize)]
struct ResolvedInstrument {
    client_instrument_id: i64,
    isin:       String,
    nse_symbol: Option<String>,
    bse_code:   Option<String>,
}

#[derive(Deserialize)]
struct ResolveResponse {
    resolved: Vec<ResolvedInstrument>,
}

#[derive(Deserialize)]
struct ServerPrice {
    isin:              String,
    price_date:        String,
    open_price_paise:  Option<i64>,
    high_price_paise:  Option<i64>,
    low_price_paise:   Option<i64>,
    close_price_paise: i64,
}

#[derive(Deserialize)]
struct SyncResponse {
    prices:    Vec<ServerPrice>,
    synced_at: Option<String>,
}

// ── DB helpers (sync, to avoid lifetime issues in async state machines) ───────

fn get_setting(key: &str) -> Option<String> {
    let conn = db::acquire().ok()?;
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        [key],
        |r| r.get::<_, String>(0),
    ).ok().filter(|v| !v.is_empty())
}

fn get_portfolio_instruments() -> Result<Vec<InstrumentRef>, String> {
    let conn = db::acquire()?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT i.instrument_id, i.isin, ie.nse_symbol, ie.bse_code
         FROM instruments i
         JOIN transactions t ON i.instrument_id = t.instrument_id
         LEFT JOIN instrument_equity ie ON ie.instrument_id = i.instrument_id",
    ).map_err(|e| e.to_string())?;

    let result = stmt.query_map([], |r| {
        Ok(InstrumentRef {
            client_instrument_id: r.get(0)?,
            isin:       r.get(1)?,
            nse_symbol: r.get(2)?,
            bse_code:   r.get(3)?,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string());
    result
}

/// Apply resolved instrument data from the server.
///
/// For each resolved ISIN:
/// - If two client instruments mapped to the same ISIN → merge them:
///   re-point all transactions to the survivor (the one that already had an ISIN,
///   or the lower-id one otherwise), copy missing NSE/BSE codes, delete the orphan.
/// - If only one client instrument mapped to the ISIN → fill in any missing fields.
///
/// Instruments that were sent but are not in `resolved` are marked UNMATCHED.
///
/// Returns (resolved_count, merged_count, unmatched_count).
fn apply_resolved(
    sent: &[InstrumentRef],
    resolved: &[ResolvedInstrument],
) -> Result<(usize, usize, usize), String> {
    let conn = db::acquire()?;

    // Group resolved entries by canonical ISIN → Vec<client_instrument_id>
    let mut isin_to_ids: HashMap<String, Vec<i64>> = HashMap::new();
    for r in resolved {
        isin_to_ids.entry(r.isin.clone()).or_default().push(r.client_instrument_id);
    }

    // Build lookup: client_instrument_id → ResolvedInstrument
    let resolved_map: HashMap<i64, &ResolvedInstrument> =
        resolved.iter().map(|r| (r.client_instrument_id, r)).collect();

    let sent_ids: std::collections::HashSet<i64> =
        sent.iter().map(|s| s.client_instrument_id).collect();

    let mut resolved_count = 0usize;
    let mut merged_count   = 0usize;

    for (isin, mut ids) in isin_to_ids {
        // Prefer the instrument that already has this ISIN as survivor
        ids.sort_unstable();
        let survivor_id = {
            let existing_isin: Option<i64> = conn.query_row(
                "SELECT instrument_id FROM instruments WHERE isin = ?1",
                [&isin],
                |r| r.get(0),
            ).ok();
            existing_isin.unwrap_or(ids[0])
        };

        // survivor_id may be an instrument that already had the ISIN in the DB
        // but wasn't returned by the server (e.g. it was sent but matched another way).
        // Fall back to any id in the group that IS in the resolved_map.
        let effective_id = if resolved_map.contains_key(&survivor_id) {
            survivor_id
        } else {
            match ids.iter().find(|id| resolved_map.contains_key(id)) {
                Some(&id) => id,
                None => continue, // nothing to apply
            }
        };
        let r = resolved_map[&effective_id];

        // Update survivor ISIN + status
        conn.execute(
            "UPDATE instruments SET isin = ?1, resolution_status = 'RESOLVED'
             WHERE instrument_id = ?2",
            rusqlite::params![isin, survivor_id],
        ).map_err(|e| e.to_string())?;

        // Merge duplicates first, before upserting survivor's equity row.
        // This is critical: orphan may hold the same bse_code/nse_symbol that the
        // survivor needs — we must delete (or copy then delete) the orphan's
        // instrument_equity row before inserting those values for the survivor,
        // otherwise the UNIQUE index on bse_code fires.
        for &orphan_id in ids.iter().filter(|&&id| id != survivor_id) {
            // Re-point all transactions
            conn.execute(
                "UPDATE transactions SET instrument_id = ?1
                 WHERE instrument_id = ?2",
                rusqlite::params![survivor_id, orphan_id],
            ).map_err(|e| e.to_string())?;

            // Re-point tax_lots
            conn.execute(
                "UPDATE tax_lots SET instrument_id = ?1
                 WHERE instrument_id = ?2",
                rusqlite::params![survivor_id, orphan_id],
            ).ok();

            // Copy missing equity identifiers from orphan to survivor BEFORE
            // deleting the orphan row (survivor may not have instrument_equity yet)
            conn.execute(
                "INSERT INTO instrument_equity (instrument_id, nse_symbol, bse_code)
                 SELECT ?1,
                        COALESCE((SELECT nse_symbol FROM instrument_equity WHERE instrument_id = ?1),
                                 nse_symbol),
                        COALESCE((SELECT bse_code FROM instrument_equity WHERE instrument_id = ?1),
                                 bse_code)
                 FROM instrument_equity WHERE instrument_id = ?2
                 ON CONFLICT(instrument_id) DO UPDATE SET
                     nse_symbol = COALESCE(nse_symbol, excluded.nse_symbol),
                     bse_code   = COALESCE(bse_code,   excluded.bse_code)",
                rusqlite::params![survivor_id, orphan_id],
            ).ok();

            // Now safe to delete orphan equity (UNIQUE index freed)
            conn.execute(
                "DELETE FROM instrument_equity WHERE instrument_id = ?1",
                [orphan_id],
            ).ok();
            conn.execute(
                "DELETE FROM latest_prices WHERE instrument_id = ?1",
                [orphan_id],
            ).ok();
            conn.execute(
                "DELETE FROM instruments WHERE instrument_id = ?1",
                [orphan_id],
            ).map_err(|e| e.to_string())?;

            merged_count += 1;
        }

        // Now that all orphan equity rows are gone, safely upsert survivor equity
        if r.nse_symbol.is_some() || r.bse_code.is_some() {
            conn.execute(
                "INSERT INTO instrument_equity (instrument_id, nse_symbol, bse_code)
                 VALUES (?1, ?2, ?3)
                 ON CONFLICT(instrument_id) DO UPDATE SET
                     nse_symbol = COALESCE(nse_symbol, excluded.nse_symbol),
                     bse_code   = COALESCE(bse_code,   excluded.bse_code)",
                rusqlite::params![survivor_id, r.nse_symbol, r.bse_code],
            ).map_err(|e| e.to_string())?;
        }

        resolved_count += 1;
    }

    // Mark instruments that were sent but not resolved as UNMATCHED
    let resolved_ids: std::collections::HashSet<i64> =
        resolved.iter().map(|r| r.client_instrument_id).collect();
    let unmatched_count = sent_ids.difference(&resolved_ids).count();
    for &id in sent_ids.difference(&resolved_ids) {
        conn.execute(
            "UPDATE instruments SET resolution_status = 'UNMATCHED'
             WHERE instrument_id = ?1 AND resolution_status = 'PENDING'",
            [id],
        ).map_err(|e| e.to_string())?;
    }

    Ok((resolved_count, merged_count, unmatched_count))
}

fn get_portfolio_isins() -> Result<Vec<String>, String> {
    let conn = db::acquire()?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT i.isin
         FROM instruments i
         JOIN transactions t ON i.instrument_id = t.instrument_id
         WHERE i.isin IS NOT NULL",
    ).map_err(|e| e.to_string())?;
    let result = stmt.query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    result
}

fn write_prices(prices: &[ServerPrice]) -> Result<usize, String> {
    let conn = db::acquire()?;
    let mut count = 0usize;
    for price in prices {
        let instrument_id: Option<i64> = conn.query_row(
            "SELECT instrument_id FROM instruments WHERE isin = ?1",
            [&price.isin],
            |r| r.get(0),
        ).ok();
        let Some(iid) = instrument_id else { continue; };

        conn.execute(
            "INSERT INTO latest_prices
                (instrument_id, price_date,
                 open_price_paise, high_price_paise, low_price_paise,
                 close_price_paise, updated_at)
             VALUES (?1, ?2, ?3, ?4, ?5, ?6, datetime('now'))
             ON CONFLICT(instrument_id) DO UPDATE SET
                 price_date        = excluded.price_date,
                 open_price_paise  = COALESCE(excluded.open_price_paise,  open_price_paise),
                 high_price_paise  = COALESCE(excluded.high_price_paise,  high_price_paise),
                 low_price_paise   = COALESCE(excluded.low_price_paise,   low_price_paise),
                 close_price_paise = excluded.close_price_paise,
                 updated_at        = datetime('now')",
            rusqlite::params![
                iid,
                price.price_date,
                price.open_price_paise,
                price.high_price_paise,
                price.low_price_paise,
                price.close_price_paise,
            ],
        ).map_err(|e| e.to_string())?;
        count += 1;
    }
    Ok(count)
}

fn save_sync_time(synced_at: &str) -> Result<(), String> {
    let conn = db::acquire()?;
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at)
         VALUES ('last_price_sync', ?1, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET
             value = excluded.value,
             updated_at = excluded.updated_at",
        [synced_at],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn resolve_server_url() -> String {
    match get_setting("server_url") {
        Some(url) if !url.contains("api.portfoliotracker.app") => url,
        _ => "http://localhost:8000".to_string(),
    }
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Resolve client instruments against the server's instrument catalog.
///
/// Sends all portfolio instruments (with whatever identifiers they have) to
/// `POST /instruments/resolve`. The server tries ISIN → NSE symbol → BSE code
/// lookup and returns canonical records.
///
/// The client then:
/// 1. Back-fills missing ISINs, NSE symbols, and BSE codes.
/// 2. Merges any duplicate instruments that resolved to the same ISIN
///    (re-points transactions, deletes the orphan).
/// 3. Marks unresolved instruments as UNMATCHED so the UI can flag them.
#[tauri::command]
pub async fn resolve_instruments() -> Result<ResolveResult, String> {
    let base_url = resolve_server_url();
    let refs     = get_portfolio_instruments()?;

    if refs.is_empty() {
        return Ok(ResolveResult { resolved: 0, merged: 0, unmatched: 0 });
    }

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(15))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .post(format!("{}/instruments/resolve", base_url))
        .json(&refs)
        .send()
        .await
        .map_err(|e| format!("Instrument server unreachable: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Instrument resolve returned {}", resp.status()));
    }

    let body: ResolveResponse = resp.json().await
        .map_err(|e| format!("Failed to parse resolve response: {}", e))?;

    let (resolved, merged, unmatched) = apply_resolved(&refs, &body.resolved)?;
    Ok(ResolveResult { resolved, merged, unmatched })
}

/// Fetch latest prices from the price server for all instruments in the
/// client's portfolio.
///
/// Sends the list of ISINs held + the last sync timestamp.
/// Server returns only the instruments whose price changed since that time.
/// Updates the local `latest_prices` table and stores the new sync timestamp.
#[tauri::command]
pub async fn sync_prices(force: Option<bool>) -> Result<SyncPricesResult, String> {
    let base_url  = resolve_server_url();
    let isins     = get_portfolio_isins()?;
    let last_sync = get_setting("last_price_sync").unwrap_or_default();

    if isins.is_empty() {
        return Ok(SyncPricesResult { updated: 0, synced_at: String::new() });
    }

    let isin_qs = isins.iter()
        .map(|i| format!("isins={}", i))
        .collect::<Vec<_>>()
        .join("&");

    // force=true or no previous sync → full fetch (no since_datetime filter)
    let skip_delta = force.unwrap_or(false) || last_sync.is_empty();
    let url = if skip_delta {
        format!("{}/prices/sync?{}", base_url, isin_qs)
    } else {
        format!("{}/prices/sync?{}&since_datetime={}", base_url, isin_qs, last_sync)
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client.get(&url)
        .send()
        .await
        .map_err(|e| format!("Price server unreachable: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Price server returned {}", resp.status()));
    }

    let body: SyncResponse = resp.json().await
        .map_err(|e| format!("Failed to parse server response: {}", e))?;

    let updated   = write_prices(&body.prices)?;
    let synced_at = body.synced_at
        .unwrap_or_else(|| chrono::Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string());

    save_sync_time(&synced_at)?;

    Ok(SyncPricesResult { updated, synced_at })
}
