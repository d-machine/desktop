use crate::db;
use chrono::{FixedOffset, NaiveTime, Utc};
use serde::{Deserialize, Serialize};

// ── Public result types ───────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct SyncPricesResult {
    pub updated:   usize,
    pub synced_at: String,
}

#[derive(Serialize)]
pub struct ResolveResult {
    pub resolved:   usize,
    pub unresolved: usize,
}

// ── Server request shape ──────────────────────────────────────────────────────

/// One item sent to POST /instruments/resolve for each pending instrument.
#[derive(Serialize)]
struct PendingRef {
    pending_id:         i64,
    instrument_type:    String,
    // equity / index / MF
    isin:               Option<String>,
    nse_symbol:         Option<String>,
    bse_code:           Option<String>,
    amfi_code:          Option<String>,
    exchange:           Option<String>,
    // F&O
    nse_fininstrmid:    Option<i64>,
    underlying_symbol:  Option<String>,
    expiry_date:        Option<String>,
    strike_price_paise: Option<i64>,
    contract_type:      Option<String>,
    // MCX
    mcx_symbol:         Option<String>,
    unit:               Option<String>,
}

// ── Server response shapes ────────────────────────────────────────────────────

/// One resolved item returned from GET /instrument-types.
#[derive(Deserialize)]
struct InstrumentTypeRow {
    instrument_type_id: i64,
    name:               String,
    asset_class:        String,
    tax_category:       String,
}

#[derive(Deserialize)]
struct InstrumentTypesResponse {
    instrument_types: Vec<InstrumentTypeRow>,
}

/// One instrument update record returned from GET /instruments/updates.
#[derive(Deserialize)]
struct InstrumentUpdate {
    instrument_id:        i64,
    name:                 String,
    instrument_type_id:   i64,
    instrument_type_name: String,
    updated_at:           String,
    isin:                 Option<String>,
    nse_symbol:           Option<String>,
    bse_code:             Option<String>,
    sector:               Option<String>,
    industry:             Option<String>,
    index_symbol:         Option<String>,
    index_exchange:       Option<String>,
    amfi_code:            Option<String>,
    mcx_symbol:           Option<String>,
}

#[derive(Deserialize)]
struct InstrumentUpdatesResponse {
    updates:   Vec<InstrumentUpdate>,
    synced_at: String,
}

/// One resolved item returned from POST /instruments/resolve.
#[derive(Deserialize)]
struct ResolvedPending {
    pending_id:               i64,
    instrument_id:            i64,
    instrument_type_id:       i64,       // server's canonical type ID — stored directly
    instrument_type_name:     String,
    name:                     String,
    primary_exchange_code:    Option<String>,
    // equity
    isin:                     Option<String>,
    nse_symbol:               Option<String>,
    nse_equity_fininstrmid:   Option<i64>,
    bse_code:                 Option<String>,
    // mutual fund
    amfi_code:                Option<String>,
    // index
    index_symbol:             Option<String>,
    index_exchange:           Option<String>,
    // derivatives (F&O)
    underlying_instrument_id: Option<i64>,
    underlying_symbol:        Option<String>,
    fo_expiry_date:           Option<String>,
    fo_lot_size:              Option<i64>,
    fo_strike_price_paise:    Option<i64>,
    fo_instrument_type:       Option<String>,   // 'FUTURES' or 'OPTIONS'
    fo_option_type:           Option<String>,   // '-', 'CE', 'PE'
    fo_nse_fininstrmid:       Option<i64>,
    fo_bse_fininstrmid:       Option<i64>,
    // MCX
    mcx_symbol:               Option<String>,
    mcx_instrument_type:      Option<String>,
    mcx_expiry_date:          Option<String>,
    mcx_lot_size:             Option<f64>,
    mcx_unit:                 Option<String>,
    mcx_strike_price_paise:   Option<i64>,
    mcx_option_type:          Option<String>,
}

#[derive(Deserialize)]
struct ResolvePendingResponse {
    resolved: Vec<ResolvedPending>,
}

/// One price record returned from GET /prices/sync keyed by instrument_id.
#[derive(Deserialize)]
struct ServerPrice {
    instrument_id:     i64,
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

// ── IST sync window ───────────────────────────────────────────────────────────

/// Returns true if the current IST time falls within an auto-sync window:
///   06:00–10:30 (pre-market) or 15:30–23:00 (post-close).
/// force=true bypasses this check entirely.
fn is_in_sync_window() -> bool {
    let ist    = FixedOffset::east_opt(5 * 3600 + 30 * 60).expect("valid offset");
    let now    = Utc::now().with_timezone(&ist).time();
    let window = |h0: u32, m0: u32, h1: u32, m1: u32| {
        let start = NaiveTime::from_hms_opt(h0, m0, 0).expect("valid time");
        let end   = NaiveTime::from_hms_opt(h1, m1, 0).expect("valid time");
        now >= start && now <= end
    };
    window(6, 0, 10, 30) || window(15, 30, 23, 0)
}

// ── DB helpers ────────────────────────────────────────────────────────────────

fn get_setting(key: &str) -> Option<String> {
    let conn = db::acquire().ok()?;
    conn.query_row(
        "SELECT value FROM app_settings WHERE key = ?1",
        [key],
        |r| r.get::<_, String>(0),
    ).ok().filter(|v| !v.is_empty())
}

fn resolve_server_url() -> String {
    get_setting("server_url").unwrap_or_else(|| "http://localhost:8000".to_string())
}

fn save_setting(key: &str, value: &str) -> Result<(), String> {
    let conn = db::acquire()?;
    conn.execute(
        "INSERT INTO app_settings (key, value, updated_at)
         VALUES (?1, ?2, datetime('now'))
         ON CONFLICT(key) DO UPDATE SET
             value      = excluded.value,
             updated_at = excluded.updated_at",
        [key, value],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

fn get_pending_instruments() -> Result<Vec<PendingRef>, String> {
    let conn = db::acquire()?;
    let mut stmt = conn.prepare(
        "SELECT pending_id,
                type                                          AS instrument_type,
                json_extract(metadata, '$.isin')             AS isin,
                json_extract(metadata, '$.nse_symbol')       AS nse_symbol,
                json_extract(metadata, '$.bse_code')         AS bse_code,
                json_extract(metadata, '$.amfi_code')        AS amfi_code,
                json_extract(metadata, '$.exchange')         AS exchange,
                json_extract(metadata, '$.underlying_symbol') AS underlying_symbol,
                json_extract(metadata, '$.expiry_date')      AS expiry_date,
                json_extract(metadata, '$.strike_price_paise') AS strike_price_paise,
                json_extract(metadata, '$.option_type')      AS contract_type,
                json_extract(metadata, '$.mcx_symbol')       AS mcx_symbol,
                json_extract(metadata, '$.unit')             AS unit
         FROM pending_instruments
         ORDER BY pending_id",
    ).map_err(|e| e.to_string())?;

    let result = stmt.query_map([], |r| {
        Ok(PendingRef {
            pending_id:         r.get(0)?,
            instrument_type:    r.get(1)?,
            isin:               r.get(2)?,
            nse_symbol:         r.get(3)?,
            bse_code:           r.get(4)?,
            amfi_code:          r.get(5)?,
            exchange:           r.get(6)?,
            nse_fininstrmid:    None,
            underlying_symbol:  r.get(7)?,
            expiry_date:        r.get(8)?,
            strike_price_paise: r.get(9)?,
            contract_type:      r.get(10)?,
            mcx_symbol:         r.get(11)?,
            unit:               r.get(12)?,
        })
    })
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string());
    result
}

fn get_portfolio_instrument_ids() -> Result<Vec<i64>, String> {
    let conn = db::acquire()?;
    let mut stmt = conn.prepare(
        "SELECT DISTINCT instrument_id
         FROM transactions
         WHERE instrument_id IS NOT NULL",
    ).map_err(|e| e.to_string())?;

    let result = stmt.query_map([], |r| r.get(0))
        .map_err(|e| e.to_string())?
        .collect::<Result<Vec<_>, _>>()
        .map_err(|e| e.to_string());
    result
}

// ── Instrument types sync ─────────────────────────────────────────────────────

async fn sync_instrument_types(
    base_url: &str,
    client: &reqwest::Client,
) -> Result<usize, String> {
    let resp = client
        .get(format!("{}/instruments/types", base_url))
        .send()
        .await
        .map_err(|e| format!("Instrument types fetch failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Instrument types returned {}", resp.status()));
    }

    let body: InstrumentTypesResponse = resp.json().await
        .map_err(|e| format!("Failed to parse instrument types: {}", e))?;

    let conn = db::acquire()?;
    let mut count = 0usize;
    for t in &body.instrument_types {
        conn.execute(
            "INSERT INTO instrument_types (instrument_type_id, name, asset_class, tax_category)
             VALUES (?1, ?2, ?3, ?4)
             ON CONFLICT(instrument_type_id) DO UPDATE SET
                 name         = excluded.name,
                 asset_class  = excluded.asset_class,
                 tax_category = excluded.tax_category",
            rusqlite::params![t.instrument_type_id, t.name, t.asset_class, t.tax_category],
        ).map_err(|e| e.to_string())?;
        count += 1;
    }
    Ok(count)
}

// ── Instrument metadata delta sync ───────────────────────────────────────────

fn apply_instrument_updates(updates: &[InstrumentUpdate]) -> Result<usize, String> {
    let conn = db::acquire()?;
    let mut count = 0usize;

    for u in updates {
        // Update base instruments row name + type
        conn.execute(
            "UPDATE instruments SET name = ?1, instrument_type_id = ?2, updated_at = ?3
             WHERE instrument_id = ?4",
            rusqlite::params![u.name, u.instrument_type_id, u.updated_at, u.instrument_id],
        ).map_err(|e| e.to_string())?;

        // Update extension table fields if present
        match u.instrument_type_name.as_str() {
            "EQUITY" => {
                conn.execute(
                    "UPDATE instrument_equity SET
                         isin       = COALESCE(?1, isin),
                         nse_symbol = COALESCE(?2, nse_symbol),
                         bse_code   = COALESCE(?3, bse_code),
                         sector     = COALESCE(?4, sector),
                         industry   = COALESCE(?5, industry)
                     WHERE instrument_id = ?6",
                    rusqlite::params![
                        u.isin, u.nse_symbol, u.bse_code,
                        u.sector, u.industry, u.instrument_id,
                    ],
                ).map_err(|e| e.to_string())?;
            }
            "INDEX" => {
                conn.execute(
                    "UPDATE instrument_index SET
                         symbol   = COALESCE(?1, symbol),
                         exchange = COALESCE(?2, exchange)
                     WHERE instrument_id = ?3",
                    rusqlite::params![u.index_symbol, u.index_exchange, u.instrument_id],
                ).map_err(|e| e.to_string())?;
            }
            "EQUITY_MF" | "DEBT_MF" | "HYBRID_MF" | "ELSS" | "SIF" => {
                conn.execute(
                    "UPDATE instrument_mf SET amfi_code = COALESCE(?1, amfi_code)
                     WHERE instrument_id = ?2",
                    rusqlite::params![u.amfi_code, u.instrument_id],
                ).map_err(|e| e.to_string())?;
            }
            "COMMODITY_FUTURES" | "COMMODITY_OPTIONS" => {
                conn.execute(
                    "UPDATE instrument_mcx SET mcx_symbol = COALESCE(?1, mcx_symbol)
                     WHERE instrument_id = ?2",
                    rusqlite::params![u.mcx_symbol, u.instrument_id],
                ).map_err(|e| e.to_string())?;
            }
            _ => {}
        }

        count += 1;
    }
    Ok(count)
}

async fn fetch_instrument_updates(
    base_url: &str,
    client: &reqwest::Client,
) -> Result<usize, String> {
    let last_sync = get_setting("last_instrument_sync").unwrap_or_default();

    let ids = get_portfolio_instrument_ids()?;
    if ids.is_empty() {
        return Ok(0);
    }

    let id_qs = ids.iter()
        .map(|id| format!("instrument_ids={}", id))
        .collect::<Vec<_>>()
        .join("&");

    let url = if last_sync.is_empty() {
        format!("{}/instruments/updates?{}", base_url, id_qs)
    } else {
        format!("{}/instruments/updates?{}&since={}", base_url, id_qs, last_sync)
    };

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Instrument updates fetch failed: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Instrument updates returned {}", resp.status()));
    }

    let body: InstrumentUpdatesResponse = resp.json().await
        .map_err(|e| format!("Failed to parse instrument updates: {}", e))?;

    let count = apply_instrument_updates(&body.updates)?;
    save_setting("last_instrument_sync", &body.synced_at)?;
    Ok(count)
}

// ── Resolve helpers ───────────────────────────────────────────────────────────

/// Apply one batch of server-resolved instruments:
/// 1. Insert into `instruments` with the server-assigned instrument_id and instrument_type_id.
/// 2. Insert into the appropriate extension table.
/// 3. Re-point all transactions from pending_instrument_id → instrument_id.
/// 4. Delete the staging row.
fn apply_resolved_pending(resolved: &[ResolvedPending]) -> Result<usize, String> {
    let conn = db::acquire()?;
    let mut count = 0usize;

    for r in resolved {
        // Map exchange code to local exchange_id
        let exchange_id: Option<i64> = r.primary_exchange_code.as_ref().and_then(|code| {
            conn.query_row(
                "SELECT exchange_id FROM exchanges WHERE code = ?1",
                [code],
                |row| row.get(0),
            ).ok()
        });

        // Insert base instruments row; uses server's canonical IDs (no AUTOINCREMENT)
        conn.execute(
            "INSERT OR IGNORE INTO instruments
                (instrument_id, name, instrument_type_id, primary_exchange_id, source)
             VALUES (?1, ?2, ?3, ?4, 'SERVER')",
            rusqlite::params![
                r.instrument_id, r.name, r.instrument_type_id, exchange_id,
            ],
        ).map_err(|e| e.to_string())?;

        // Insert into the matching extension table
        match r.instrument_type_name.as_str() {
            "EQUITY" => {
                conn.execute(
                    "INSERT OR IGNORE INTO instrument_equity
                        (instrument_id, isin, nse_symbol, nse_fininstrmid, bse_code)
                     VALUES (?1, ?2, ?3, ?4, ?5)",
                    rusqlite::params![
                        r.instrument_id, r.isin, r.nse_symbol,
                        r.nse_equity_fininstrmid, r.bse_code,
                    ],
                ).map_err(|e| e.to_string())?;
            }
            "INDEX" => {
                conn.execute(
                    "INSERT OR IGNORE INTO instrument_index
                        (instrument_id, symbol, exchange)
                     VALUES (?1, ?2, ?3)",
                    rusqlite::params![
                        r.instrument_id, r.index_symbol, r.index_exchange,
                    ],
                ).map_err(|e| e.to_string())?;
            }
            "EQUITY_MF" | "DEBT_MF" | "HYBRID_MF" | "ELSS" | "SIF" => {
                conn.execute(
                    "INSERT OR IGNORE INTO instrument_mf
                        (instrument_id, amfi_code)
                     VALUES (?1, ?2)",
                    rusqlite::params![r.instrument_id, r.amfi_code],
                ).map_err(|e| e.to_string())?;
            }
            "FUTURES" | "OPTIONS" => {
                conn.execute(
                    "INSERT OR IGNORE INTO instrument_derivatives
                        (instrument_id, underlying_instrument_id, underlying_symbol,
                         expiry_date, lot_size, strike_price_paise,
                         instrument_type, option_type,
                         nse_fininstrmid, bse_fininstrmid)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8, ?9, ?10)",
                    rusqlite::params![
                        r.instrument_id,
                        r.underlying_instrument_id,
                        r.underlying_symbol,
                        r.fo_expiry_date,
                        r.fo_lot_size.unwrap_or(0),
                        r.fo_strike_price_paise,
                        r.fo_instrument_type,
                        r.fo_option_type,
                        r.fo_nse_fininstrmid,
                        r.fo_bse_fininstrmid,
                    ],
                ).map_err(|e| e.to_string())?;
            }
            "COMMODITY_FUTURES" | "COMMODITY_OPTIONS" => {
                conn.execute(
                    "INSERT OR IGNORE INTO instrument_mcx
                        (instrument_id, mcx_symbol, instrument_type, expiry_date,
                         lot_size, unit, strike_price_paise, option_type)
                     VALUES (?1, ?2, ?3, ?4, ?5, ?6, ?7, ?8)",
                    rusqlite::params![
                        r.instrument_id,
                        r.mcx_symbol,
                        r.mcx_instrument_type,
                        r.mcx_expiry_date,
                        r.mcx_lot_size.unwrap_or(0.0),
                        r.mcx_unit,
                        r.mcx_strike_price_paise,
                        r.mcx_option_type,
                    ],
                ).map_err(|e| e.to_string())?;
            }
            _ => {}
        }

        // Re-point transactions: pending_instrument_id → resolved instrument_id
        conn.execute(
            "UPDATE transactions
             SET instrument_id = ?1, pending_instrument_id = NULL
             WHERE pending_instrument_id = ?2",
            rusqlite::params![r.instrument_id, r.pending_id],
        ).map_err(|e| e.to_string())?;

        // Remove from staging table
        conn.execute(
            "DELETE FROM pending_instruments WHERE pending_id = ?1",
            [r.pending_id],
        ).map_err(|e| e.to_string())?;

        count += 1;
    }

    Ok(count)
}

fn write_prices(prices: &[ServerPrice]) -> Result<usize, String> {
    let conn = db::acquire()?;
    let mut count = 0usize;
    for p in prices {
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
                p.instrument_id,
                p.price_date,
                p.open_price_paise,
                p.high_price_paise,
                p.low_price_paise,
                p.close_price_paise,
            ],
        ).map_err(|e| e.to_string())?;
        count += 1;
    }
    Ok(count)
}

// ── Commands ──────────────────────────────────────────────────────────────────

/// Resolve all pending instruments against the server's instrument catalog.
///
/// Flow:
///   1. Sync instrument_types from server (always first — safety net for new types).
///   2. Send pending_instruments to POST /instruments/resolve.
///   3. If any returned instrument_type_id is missing locally, re-sync types and retry.
///   4. Apply resolved instruments to local DB.
#[tauri::command]
pub async fn resolve_instruments() -> Result<ResolveResult, String> {
    let base_url = resolve_server_url();
    let pending  = get_pending_instruments()?;

    if pending.is_empty() {
        return Ok(ResolveResult { resolved: 0, unresolved: 0 });
    }

    let total = pending.len();

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    // Always sync instrument types first
    sync_instrument_types(&base_url, &client).await?;

    let resp = client
        .post(format!("{}/instruments/resolve", base_url))
        .json(&pending)
        .send()
        .await
        .map_err(|e| format!("Instrument server unreachable: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Instrument resolve returned {}", resp.status()));
    }

    let body: ResolvePendingResponse = resp.json().await
        .map_err(|e| format!("Failed to parse resolve response: {}", e))?;

    // Safety net: if any returned type_id is not yet local, re-sync types.
    // conn must be dropped before the await — MutexGuard is not Send.
    let needs_type_resync = {
        let conn = db::acquire()?;
        body.resolved.iter().any(|r| {
            conn.query_row(
                "SELECT 1 FROM instrument_types WHERE instrument_type_id = ?1",
                [r.instrument_type_id],
                |_| Ok(()),
            ).is_err()
        })
    };
    if needs_type_resync {
        sync_instrument_types(&base_url, &client).await?;
    }

    let resolved = apply_resolved_pending(&body.resolved)?;

    Ok(ResolveResult {
        resolved,
        unresolved: total.saturating_sub(resolved),
    })
}

/// Fetch latest prices from the server for all resolved portfolio instruments.
///
/// Flow:
///   1. IST window check (bypass with force=true).
///   2. Fetch prices for all portfolio instrument_ids.
///   3. Best-effort instrument metadata delta sync.
///   4. Save last_price_sync + last_instrument_sync timestamps.
#[tauri::command]
pub async fn sync_prices(force: Option<bool>) -> Result<SyncPricesResult, String> {
    let force_sync = force.unwrap_or(false);

    if !force_sync && !is_in_sync_window() {
        return Ok(SyncPricesResult { updated: 0, synced_at: String::new() });
    }

    let base_url  = resolve_server_url();
    let ids       = get_portfolio_instrument_ids()?;
    let last_sync = get_setting("last_price_sync").unwrap_or_default();

    if ids.is_empty() {
        return Ok(SyncPricesResult { updated: 0, synced_at: String::new() });
    }

    let id_qs = ids.iter()
        .map(|id| format!("instrument_ids={}", id))
        .collect::<Vec<_>>()
        .join("&");

    let url = if force_sync || last_sync.is_empty() {
        format!("{}/prices/sync?{}", base_url, id_qs)
    } else {
        format!("{}/prices/sync?{}&since_datetime={}", base_url, id_qs, last_sync)
    };

    let client = reqwest::Client::builder()
        .timeout(std::time::Duration::from_secs(30))
        .build()
        .map_err(|e| e.to_string())?;

    let resp = client
        .get(&url)
        .send()
        .await
        .map_err(|e| format!("Price server unreachable: {}", e))?;

    if !resp.status().is_success() {
        return Err(format!("Price server returned {}", resp.status()));
    }

    let body: SyncResponse = resp.json().await
        .map_err(|e| format!("Failed to parse price response: {}", e))?;

    let updated   = write_prices(&body.prices)?;
    let synced_at = body.synced_at
        .unwrap_or_else(|| Utc::now().format("%Y-%m-%dT%H:%M:%S").to_string());

    save_setting("last_price_sync", &synced_at)?;

    // Best-effort instrument metadata delta sync — don't fail price sync on error
    let _ = fetch_instrument_updates(&base_url, &client).await;

    Ok(SyncPricesResult { updated, synced_at })
}
