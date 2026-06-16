use crate::db;
use crate::commands::import::{common, flag_oversells};
use serde::{Deserialize, Serialize};
use serde_json::json;

#[derive(Serialize)]
pub struct Transaction {
    pub txn_id: i64,
    pub account_id: i64,
    pub account_name: String,
    pub portfolio_id: i64,
    pub instrument_id: i64,
    pub instrument_name: String,
    pub isin: Option<String>,
    pub txn_type: String,
    pub trade_segment: String,
    pub trade_date: String,
    pub txn_time: Option<String>,
    pub quantity: f64,
    pub price_paise: i64,
    pub brokerage_paise: i64,
    pub stt_paise: i64,
    pub other_charges_paise: i64,
    pub total_value_paise: i64,
    pub notes: Option<String>,
    pub broker_ref: Option<String>,
    // Flag fields
    pub flag: Option<String>,
    pub flag_reason: Option<String>,
    pub flag_dismissed: bool,
    pub batch_id: Option<i64>,
}

#[derive(Deserialize)]
pub struct CreateTransactionInput {
    pub account_id:          i64,
    /// Resolved instrument — mutually exclusive with the two pending options.
    pub instrument_id:       Option<i64>,
    /// Link to an *existing* pending_instruments row (selected from search).
    /// No new row is created — we just reference the existing pending_id.
    pub existing_pending_id: Option<i64>,
    /// New pending instrument to stage — creates a new pending_instruments row.
    pub pending_instrument:  Option<common::PendingInstrumentSpec>,
    pub txn_type:            String,
    pub trade_segment:      String,
    pub trade_date:         String,
    pub txn_time:           Option<String>,
    pub quantity:           f64,
    pub price_paise:        i64,
    pub brokerage_paise:    i64,
    pub stt_paise:          i64,
    pub other_charges_paise: i64,
    pub notes:              Option<String>,
    pub broker_ref:         Option<String>,
}

#[derive(Deserialize)]
pub struct UpdateTransactionInput {
    pub txn_id: i64,
    pub txn_type: String,
    pub trade_segment: String,
    pub trade_date: String,
    pub txn_time: Option<String>,
    pub quantity: f64,
    pub price_paise: i64,
    pub brokerage_paise: i64,
    pub stt_paise: i64,
    pub other_charges_paise: i64,
    pub notes: Option<String>,
}

#[derive(Deserialize)]
pub struct GetTransactionsFilter {
    pub account_ids:  Option<Vec<i64>>,
    pub instrument_id: Option<i64>,
    pub from_date:    Option<String>,
    pub to_date:      Option<String>,
    pub txn_type:     Option<String>,
    /// "all" | "flagged" | "clean"
    pub flag_filter:  Option<String>,
    /// Free-text search matched against instrument name, account name, txn_type
    pub search:       Option<String>,
    /// Column to sort by: "trade_date" | "instrument_name" | "quantity" | "price_paise" | "total_value_paise"
    pub sort_col:     Option<String>,
    /// "asc" | "desc"
    pub sort_dir:     Option<String>,
    pub limit:        Option<i64>,
    pub offset:       Option<i64>,
}

#[tauri::command]
pub fn get_transactions(filter: GetTransactionsFilter) -> Result<Vec<Transaction>, String> {
    let conn = db::acquire()?;

    let mut sql = String::from(
        "SELECT t.txn_id, t.account_id, a.name AS account_name,
                a.portfolio_id,
                COALESCE(t.instrument_id, -t.pending_instrument_id) AS instrument_id,
                COALESCE(i.name, pi.name)                           AS instrument_name,
                ie.isin,
                t.txn_type, t.trade_segment, t.trade_date, t.txn_time,
                t.quantity, t.price_paise, t.brokerage_paise,
                t.stt_paise, t.other_charges_paise, t.total_value_paise,
                t.notes, t.broker_ref,
                t.flag, t.flag_reason, t.flag_dismissed,
                t.batch_id
         FROM transactions t
         JOIN accounts a ON t.account_id = a.account_id
         LEFT JOIN instruments i ON i.instrument_id = t.instrument_id
         LEFT JOIN pending_instruments pi ON pi.pending_id = t.pending_instrument_id
         LEFT JOIN instrument_equity ie ON ie.instrument_id = t.instrument_id
         WHERE 1=1",
    );

    if filter.account_ids.as_ref().map(|v| !v.is_empty()).unwrap_or(false) {
        let placeholders = filter.account_ids.as_ref().unwrap()
            .iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>().join(",");
        sql.push_str(&format!(" AND t.account_id IN ({})", placeholders));
    }
    if filter.from_date.is_some()    { sql.push_str(" AND t.trade_date >= ?"); }
    if filter.to_date.is_some()      { sql.push_str(" AND t.trade_date <= ?"); }
    if filter.txn_type.is_some()     { sql.push_str(" AND t.txn_type = ?"); }
    if let Some(id) = filter.instrument_id {
        if id < 0 {
            sql.push_str(" AND t.pending_instrument_id = ?");
        } else {
            sql.push_str(" AND t.instrument_id = ?");
        }
    }

    match filter.flag_filter.as_deref() {
        Some("flagged") => sql.push_str(" AND t.flag IS NOT NULL AND t.flag_dismissed = 0"),
        Some("clean")   => sql.push_str(" AND (t.flag IS NULL OR t.flag_dismissed = 1)"),
        _               => {}
    }
    if filter.search.as_ref().map(|s| !s.is_empty()).unwrap_or(false) {
        sql.push_str(
            " AND (COALESCE(i.name, pi.name) LIKE ? OR a.name LIKE ? OR t.txn_type LIKE ?)"
        );
    }

    let sort_col = match filter.sort_col.as_deref() {
        Some("instrument_name")   => "COALESCE(i.name, pi.name)",
        Some("quantity")          => "t.quantity",
        Some("price_paise")       => "t.price_paise",
        Some("total_value_paise") => "t.total_value_paise",
        _                         => "t.trade_date",
    };
    let sort_dir = if filter.sort_dir.as_deref() == Some("asc") { "ASC" } else { "DESC" };
    sql.push_str(&format!(" ORDER BY {} {}, t.txn_id DESC", sort_col, sort_dir));

    match (filter.limit, filter.offset) {
        (Some(limit), offset) => sql.push_str(&format!(" LIMIT {} OFFSET {}", limit, offset.unwrap_or(0))),
        (None, Some(offset))  => sql.push_str(&format!(" LIMIT -1 OFFSET {}", offset)),
        (None, None)          => {}
    }

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];
    if let Some(ids) = &filter.account_ids {
        for id in ids { params.push(Box::new(*id)); }
    }
    if let Some(d) = &filter.from_date   { params.push(Box::new(d.clone())); }
    if let Some(d) = &filter.to_date     { params.push(Box::new(d.clone())); }
    if let Some(t) = &filter.txn_type    { params.push(Box::new(t.clone())); }
    if let Some(id) = filter.instrument_id {
        if id < 0 { params.push(Box::new(-id)); } else { params.push(Box::new(id)); }
    }
    if let Some(s) = &filter.search {
        if !s.is_empty() {
            let term = format!("%{}%", s);
            params.push(Box::new(term.clone()));
            params.push(Box::new(term.clone()));
            params.push(Box::new(term));
        }
    }

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let txns = stmt.query_map(
        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
        map_row,
    )
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    Ok(txns)
}

/// Returns the total number of transactions matching the same filter as `get_transactions`
/// (without limit/offset/sort). Used by the frontend for pagination controls.
#[tauri::command]
pub fn get_transactions_count(filter: GetTransactionsFilter) -> Result<i64, String> {
    let conn = db::acquire()?;

    let mut sql = String::from(
        "SELECT COUNT(*)
         FROM transactions t
         JOIN accounts a ON t.account_id = a.account_id
         LEFT JOIN instruments i ON i.instrument_id = t.instrument_id
         LEFT JOIN pending_instruments pi ON pi.pending_id = t.pending_instrument_id
         LEFT JOIN instrument_equity ie ON ie.instrument_id = t.instrument_id
         WHERE 1=1",
    );

    if filter.account_ids.as_ref().map(|v| !v.is_empty()).unwrap_or(false) {
        let placeholders = filter.account_ids.as_ref().unwrap()
            .iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>().join(",");
        sql.push_str(&format!(" AND t.account_id IN ({})", placeholders));
    }
    if filter.from_date.is_some()    { sql.push_str(" AND t.trade_date >= ?"); }
    if filter.to_date.is_some()      { sql.push_str(" AND t.trade_date <= ?"); }
    if filter.txn_type.is_some()     { sql.push_str(" AND t.txn_type = ?"); }
    if let Some(id) = filter.instrument_id {
        if id < 0 { sql.push_str(" AND t.pending_instrument_id = ?"); }
        else       { sql.push_str(" AND t.instrument_id = ?"); }
    }
    match filter.flag_filter.as_deref() {
        Some("flagged") => sql.push_str(" AND t.flag IS NOT NULL AND t.flag_dismissed = 0"),
        Some("clean")   => sql.push_str(" AND (t.flag IS NULL OR t.flag_dismissed = 1)"),
        _               => {}
    }
    if filter.search.as_ref().map(|s| !s.is_empty()).unwrap_or(false) {
        sql.push_str(
            " AND (COALESCE(i.name, pi.name) LIKE ? OR a.name LIKE ? OR t.txn_type LIKE ?)"
        );
    }

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];
    if let Some(ids) = &filter.account_ids {
        for id in ids { params.push(Box::new(*id)); }
    }
    if let Some(d) = &filter.from_date   { params.push(Box::new(d.clone())); }
    if let Some(d) = &filter.to_date     { params.push(Box::new(d.clone())); }
    if let Some(t) = &filter.txn_type    { params.push(Box::new(t.clone())); }
    if let Some(id) = filter.instrument_id {
        if id < 0 { params.push(Box::new(-id)); } else { params.push(Box::new(id)); }
    }
    if let Some(s) = &filter.search {
        if !s.is_empty() {
            let term = format!("%{}%", s);
            params.push(Box::new(term.clone()));
            params.push(Box::new(term.clone()));
            params.push(Box::new(term));
        }
    }

    conn.query_row(
        &sql,
        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
        |row| row.get(0),
    ).map_err(|e| e.to_string())
}

/// Returns the count of flagged, non-dismissed transactions for the given accounts.
/// Counts across all pages — not limited to the current page.
#[tauri::command]
pub fn get_flagged_count(account_ids: Option<Vec<i64>>) -> Result<i64, String> {
    let conn = db::acquire()?;
    let mut sql = String::from(
        "SELECT COUNT(*) FROM transactions t
         WHERE t.flag IS NOT NULL AND t.flag_dismissed = 0",
    );
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];
    if let Some(ids) = &account_ids {
        if !ids.is_empty() {
            let ph = ids.iter().enumerate()
                .map(|(i, _)| format!("?{}", i + 1))
                .collect::<Vec<_>>().join(",");
            sql.push_str(&format!(" AND t.account_id IN ({})", ph));
            for id in ids { params.push(Box::new(*id)); }
        }
    }
    conn.query_row(
        &sql,
        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
        |row| row.get(0),
    ).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn create_transaction(input: CreateTransactionInput) -> Result<Transaction, String> {
    // Resolve instrument_id / pending_instrument_id
    let (instrument_id, pending_instrument_id): (Option<i64>, Option<i64>) =
        if let Some(id) = input.instrument_id {
            (Some(id), None)
        } else if let Some(pid) = input.existing_pending_id {
            (None, Some(pid))
        } else if let Some(spec) = &input.pending_instrument {
            let pid = common::create_pending_instrument(spec)?;
            (None, Some(pid))
        } else {
            return Err("instrument_id, existing_pending_id, or pending_instrument must be provided".to_string());
        };

    let conn = db::acquire()?;

    let gross   = (input.quantity * input.price_paise as f64) as i64;
    let charges = input.brokerage_paise + input.stt_paise + input.other_charges_paise;
    let total_value_paise = match input.txn_type.as_str() {
        "BUY" | "SIP" | "IPO" | "FPO" | "OPENING_BALANCE"
        | "TRANSFER_IN" | "SPLIT_IN" | "MERGER_IN" | "SWITCH_IN" => -(gross + charges),
        "SELL" | "REDEMPTION" | "TRANSFER_OUT" | "SPLIT_OUT"
        | "MERGER_OUT" | "SWITCH_OUT"                            =>   gross - charges,
        "DIVIDEND" | "INTEREST"                                   =>   gross,
        _                                                         =>   gross,
    };

    conn.execute(
        "INSERT INTO transactions
            (account_id, instrument_id, pending_instrument_id, txn_type, trade_segment,
             trade_date, txn_time, quantity, price_paise, brokerage_paise, stt_paise,
             other_charges_paise, total_value_paise, notes, broker_ref)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14,?15)",
        rusqlite::params![
            input.account_id, instrument_id, pending_instrument_id, input.txn_type,
            input.trade_segment, input.trade_date, input.txn_time,
            input.quantity, input.price_paise, input.brokerage_paise,
            input.stt_paise, input.other_charges_paise,
            total_value_paise, input.notes, input.broker_ref,
        ],
    ).map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    let txn = get_transaction_by_id(&conn, id).map_err(|e| e.to_string())?;

    drop(conn);
    flag_oversells(txn.account_id)?;

    let conn2 = db::acquire()?;
    get_transaction_by_id(&conn2, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn update_transaction(input: UpdateTransactionInput) -> Result<Transaction, String> {
    let account_id = {
        let conn = db::acquire()?;
        let aid: i64 = conn.query_row(
            "SELECT account_id FROM transactions WHERE txn_id = ?1",
            [input.txn_id],
            |r| r.get(0),
        ).map_err(|e| e.to_string())?;
        aid
    };

    let conn = db::acquire()?;

    let gross   = (input.quantity * input.price_paise as f64) as i64;
    let charges = input.brokerage_paise + input.stt_paise + input.other_charges_paise;
    let total_value_paise = match input.txn_type.as_str() {
        "BUY" | "SIP" | "IPO" | "FPO" | "OPENING_BALANCE"
        | "TRANSFER_IN" | "SPLIT_IN" | "MERGER_IN" | "SWITCH_IN" => -(gross + charges),
        "SELL" | "REDEMPTION" | "TRANSFER_OUT" | "SPLIT_OUT"
        | "MERGER_OUT" | "SWITCH_OUT"                            =>   gross - charges,
        "DIVIDEND" | "INTEREST"                                   =>   gross,
        _                                                         =>   gross,
    };

    conn.execute(
        "UPDATE transactions SET
            txn_type=?1, trade_segment=?2, trade_date=?3, txn_time=?4,
            quantity=?5, price_paise=?6, brokerage_paise=?7, stt_paise=?8,
            other_charges_paise=?9, total_value_paise=?10, notes=?11,
            flag=NULL, flag_reason=NULL, flag_dismissed=0
         WHERE txn_id=?12",
        rusqlite::params![
            input.txn_type, input.trade_segment, input.trade_date, input.txn_time,
            input.quantity, input.price_paise, input.brokerage_paise,
            input.stt_paise, input.other_charges_paise, total_value_paise,
            input.notes, input.txn_id,
        ],
    ).map_err(|e| e.to_string())?;

    drop(conn);

    // Re-evaluate flags for the account after the edit
    flag_oversells(account_id)?;

    let conn2 = db::acquire()?;
    get_transaction_by_id(&conn2, input.txn_id).map_err(|e| e.to_string())
}

/// Mark a flagged transaction as dismissed — user acknowledges the issue
/// and wants it included in portfolio calculations anyway.
#[tauri::command]
pub fn dismiss_transaction_flag(txn_id: i64) -> Result<(), String> {
    let conn = db::acquire()?;
    conn.execute(
        "UPDATE transactions SET flag_dismissed=1 WHERE txn_id=?1",
        [txn_id],
    ).map_err(|e| e.to_string())?;
    Ok(())
}

/// Re-run oversell detection for all accounts (or a specific account).
/// Call this after manually adding opening balances or editing transactions.
#[tauri::command]
pub fn re_evaluate_flags(account_id: Option<i64>) -> Result<(), String> {
    let conn = db::acquire()?;
    let ids: Vec<i64> = match account_id {
        Some(id) => vec![id],
        None => {
            conn.prepare("SELECT account_id FROM accounts")
                .map_err(|e| e.to_string())?
                .query_map([], |r| r.get(0))
                .map_err(|e| e.to_string())?
                .collect::<Result<Vec<_>, _>>()
                .map_err(|e| e.to_string())?
        }
    };
    drop(conn);

    for id in ids {
        flag_oversells(id)?;
    }
    Ok(())
}

#[tauri::command]
pub fn delete_transaction(txn_id: i64) -> Result<(), String> {
    let account_id = {
        let conn = db::acquire()?;
        let aid: i64 = conn.query_row(
            "SELECT account_id FROM transactions WHERE txn_id = ?1",
            [txn_id],
            |r| r.get(0),
        ).map_err(|e| e.to_string())?;
        aid
    };

    let conn = db::acquire()?;
    conn.execute("DELETE FROM transactions WHERE txn_id = ?1", [txn_id])
        .map_err(|e| e.to_string())?;
    drop(conn);

    flag_oversells(account_id)?;
    Ok(())
}

#[derive(Deserialize)]
pub struct TransferHoldingInput {
    pub from_account_id: i64,
    pub to_account_id:   i64,
    pub instrument_id:   i64,
    pub quantity:        f64,
    pub price_paise:     i64,
    pub trade_date:      String,
    pub trade_segment:   String,
    pub notes:           Option<String>,
}

#[derive(Serialize)]
pub struct TransferResult {
    pub transfer_out_txn_id: i64,
    pub transfer_in_txn_id:  i64,
}

#[tauri::command]
pub fn transfer_holding(input: TransferHoldingInput) -> Result<TransferResult, String> {
    let gross = (input.quantity * input.price_paise as f64) as i64;
    // OUT: positive total (proceeds), IN: negative total (cost)
    let total_out =  gross;
    let total_in  = -gross;

    // Generate a shared reference for both legs so they can be linked via broker_ref.
    // Format: TRF-{unix_ms} — unique enough for manual transfers.
    let pair_ref = format!(
        "TRF-{}",
        std::time::SystemTime::now()
            .duration_since(std::time::UNIX_EPOCH)
            .unwrap_or_default()
            .as_millis()
    );

    let (out_id, in_id) = {
        let conn = db::acquire()?;
        conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;

        conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, txn_type, trade_segment, trade_date,
                 quantity, price_paise, brokerage_paise, stt_paise, other_charges_paise,
                 total_value_paise, notes, broker_ref)
             VALUES (?1,?2,'TRANSFER_OUT',?3,?4,?5,?6,0,0,0,?7,?8,?9)",
            rusqlite::params![
                input.from_account_id, input.instrument_id,
                input.trade_segment, input.trade_date,
                input.quantity, input.price_paise, total_out, input.notes, pair_ref,
            ],
        ).map_err(|e| { let _ = conn.execute_batch("ROLLBACK"); e.to_string() })?;
        let out_id = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, txn_type, trade_segment, trade_date,
                 quantity, price_paise, brokerage_paise, stt_paise, other_charges_paise,
                 total_value_paise, notes, broker_ref)
             VALUES (?1,?2,'TRANSFER_IN',?3,?4,?5,?6,0,0,0,?7,?8,?9)",
            rusqlite::params![
                input.to_account_id, input.instrument_id,
                input.trade_segment, input.trade_date,
                input.quantity, input.price_paise, total_in, input.notes, pair_ref,
            ],
        ).map_err(|e| { let _ = conn.execute_batch("ROLLBACK"); e.to_string() })?;
        let in_id = conn.last_insert_rowid();

        let ca_data = json!({
            "type":               "TRANSFER",
            "transfer_date":      input.trade_date,
            "from_account_id":    input.from_account_id,
            "to_account_id":      input.to_account_id,
            "instrument_id":      input.instrument_id,
            "quantity":           input.quantity,
            "price_paise":        input.price_paise,
            "transfer_out_txn_id": out_id,
            "transfer_in_txn_id":  in_id,
        }).to_string();
        conn.execute(
            "INSERT INTO corporate_actions (data) VALUES (?1)",
            [&ca_data],
        ).map_err(|e| { let _ = conn.execute_batch("ROLLBACK"); e.to_string() })?;

        conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
        (out_id, in_id)
    };

    flag_oversells(input.from_account_id)?;
    flag_oversells(input.to_account_id)?;

    Ok(TransferResult { transfer_out_txn_id: out_id, transfer_in_txn_id: in_id })
}

#[derive(Deserialize)]
pub struct CreateSplitInput {
    pub account_id:            i64,
    /// Unified instrument_id from the holdings view: positive = resolved, negative = pending.
    pub from_instrument_id:    i64,
    /// Resolved instrument for the post-split instrument, if already in the catalog.
    pub to_instrument_id:      Option<i64>,
    /// Create a new pending instrument for the post-split side (mutually exclusive with to_instrument_id).
    pub to_pending:            Option<common::PendingInstrumentSpec>,
    pub qty_before:            f64,
    pub qty_after:             f64,
    /// User-supplied avg cost per share for the pre-split position (in paise).
    /// Used to compute the post-split per-share cost so total cost basis is preserved.
    pub avg_cost_before_paise: i64,
    pub trade_date:            String,
    pub notes:                 Option<String>,
}

#[derive(Serialize)]
pub struct CreateSplitResult {
    pub ca_id:            i64,
    pub split_out_txn_id: i64,
    pub split_in_txn_id:  i64,
}

/// Record a stock split / ISIN succession as a SPLIT_OUT + SPLIT_IN pair,
/// with an audit row in corporate_actions.
///
/// Cost basis is preserved: total_cost = qty_before × avg_cost_before.
/// The SPLIT_IN price is set to total_cost / qty_after so the position
/// carries the same total cost under the new instrument.
#[tauri::command]
pub fn create_split(input: CreateSplitInput) -> Result<CreateSplitResult, String> {
    // Decode from_instrument (positive = instrument_id, negative = pending_id)
    let (from_instr_id, from_pending_id): (Option<i64>, Option<i64>) =
        if input.from_instrument_id > 0 {
            (Some(input.from_instrument_id), None)
        } else {
            (None, Some(-input.from_instrument_id))
        };

    // Resolve to_instrument
    let (to_instr_id, to_pending_id): (Option<i64>, Option<i64>) =
        if let Some(id) = input.to_instrument_id {
            (Some(id), None)
        } else if let Some(spec) = &input.to_pending {
            let pid = common::create_pending_instrument(spec)?;
            (None, Some(pid))
        } else {
            return Err("to_instrument_id or to_pending must be provided".to_string());
        };

    let total_cost_paise = (input.qty_before * input.avg_cost_before_paise as f64).round() as i64;
    let price_after_paise = if input.qty_after > 0.0 {
        (total_cost_paise as f64 / input.qty_after).round() as i64
    } else { 0 };

    let (out_id, in_id, ca_id) = {
        let conn = db::acquire()?;
        conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;

        conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, pending_instrument_id,
                 txn_type, trade_segment, trade_date,
                 quantity, price_paise, brokerage_paise, stt_paise,
                 other_charges_paise, total_value_paise, notes)
             VALUES (?1,?2,?3,'SPLIT_OUT','DELIVERY',?4,?5,?6,0,0,0,?7,?8)",
            rusqlite::params![
                input.account_id, from_instr_id, from_pending_id,
                input.trade_date,
                input.qty_before, input.avg_cost_before_paise,
                total_cost_paise,   // positive = proceeds (sell-like)
                input.notes,
            ],
        ).map_err(|e| { let _ = conn.execute_batch("ROLLBACK"); e.to_string() })?;
        let out_id = conn.last_insert_rowid();

        conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, pending_instrument_id,
                 txn_type, trade_segment, trade_date,
                 quantity, price_paise, brokerage_paise, stt_paise,
                 other_charges_paise, total_value_paise, notes)
             VALUES (?1,?2,?3,'SPLIT_IN','DELIVERY',?4,?5,?6,0,0,0,?7,?8)",
            rusqlite::params![
                input.account_id, to_instr_id, to_pending_id,
                input.trade_date,
                input.qty_after, price_after_paise,
                -total_cost_paise,  // negative = cost (buy-like)
                input.notes,
            ],
        ).map_err(|e| { let _ = conn.execute_batch("ROLLBACK"); e.to_string() })?;
        let in_id = conn.last_insert_rowid();

        let ca_data = json!({
            "type":                  "SPLIT",
            "ex_date":               input.trade_date,
            "account_id":            input.account_id,
            "from_instrument_id":    input.from_instrument_id,
            "to_instrument_id":      input.to_instrument_id,
            "qty_before":            input.qty_before,
            "qty_after":             input.qty_after,
            "avg_cost_before_paise": input.avg_cost_before_paise,
            "split_out_txn_id":      out_id,
            "split_in_txn_id":       in_id,
        }).to_string();
        conn.execute(
            "INSERT INTO corporate_actions (data) VALUES (?1)",
            [&ca_data],
        ).map_err(|e| { let _ = conn.execute_batch("ROLLBACK"); e.to_string() })?;
        let ca_id = conn.last_insert_rowid();

        conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
        (out_id, in_id, ca_id)
    };

    flag_oversells(input.account_id)?;

    Ok(CreateSplitResult { ca_id, split_out_txn_id: out_id, split_in_txn_id: in_id })
}

fn get_transaction_by_id(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<Transaction> {
    conn.query_row(
        "SELECT t.txn_id, t.account_id, a.name, a.portfolio_id,
                COALESCE(t.instrument_id, -t.pending_instrument_id) AS instrument_id,
                COALESCE(i.name, pi.name)                           AS instrument_name,
                ie.isin,
                t.txn_type, t.trade_segment, t.trade_date, t.txn_time,
                t.quantity, t.price_paise, t.brokerage_paise,
                t.stt_paise, t.other_charges_paise, t.total_value_paise,
                t.notes, t.broker_ref,
                t.flag, t.flag_reason, t.flag_dismissed,
                t.batch_id
         FROM transactions t
         JOIN accounts a ON t.account_id = a.account_id
         LEFT JOIN instruments i ON i.instrument_id = t.instrument_id
         LEFT JOIN pending_instruments pi ON pi.pending_id = t.pending_instrument_id
         LEFT JOIN instrument_equity ie ON ie.instrument_id = t.instrument_id
         WHERE t.txn_id = ?1",
        [id],
        map_row,
    )
}

fn map_row(row: &rusqlite::Row<'_>) -> rusqlite::Result<Transaction> {
    Ok(Transaction {
        txn_id:               row.get(0)?,
        account_id:           row.get(1)?,
        account_name:         row.get(2)?,
        portfolio_id:         row.get(3)?,
        instrument_id:        row.get(4)?,
        instrument_name:      row.get(5)?,
        isin:                 row.get(6)?,
        txn_type:             row.get(7)?,
        trade_segment:        row.get(8)?,
        trade_date:           row.get(9)?,
        txn_time:             row.get(10)?,
        quantity:             row.get(11)?,
        price_paise:          row.get(12)?,
        brokerage_paise:      row.get(13)?,
        stt_paise:            row.get(14)?,
        other_charges_paise:  row.get(15)?,
        total_value_paise:    row.get(16)?,
        notes:                row.get(17)?,
        broker_ref:           row.get(18)?,
        flag:                 row.get(19)?,
        flag_reason:          row.get(20)?,
        flag_dismissed:       row.get::<_, i64>(21).map(|v| v != 0)?,
        batch_id:             row.get(22)?,
    })
}

// ─── Import batch detail ──────────────────────────────────────────────────────

#[derive(Serialize)]
pub struct ImportBatch {
    pub batch_id:            i64,
    pub account_id:          i64,
    pub source_type:         String,
    pub file_name:           Option<String>,
    pub ref_no:              Option<String>,
    pub broker:              Option<String>,
    pub batch_trade_date:    Option<String>,
    pub imported_at:         String,
    pub record_count:        i64,
    pub stt_paise:           i64,
    pub stamp_charges_paise: i64,
    pub gst_paise:           i64,
    pub trans_charges_paise: i64,
    pub other_charges_paise: i64,
    pub total_payable_paise: i64,
    pub transactions:        Vec<Transaction>,
}

/// Fetch a single import batch with all its transactions.
#[tauri::command]
pub fn get_import_batch(batch_id: i64) -> Result<ImportBatch, String> {
    let conn = db::acquire()?;

    let batch: ImportBatch = conn.query_row(
        "SELECT batch_id, account_id, source_type, file_name, ref_no, broker, batch_trade_date,
                imported_at, record_count,
                COALESCE(stt_paise,0), COALESCE(stamp_charges_paise,0),
                COALESCE(gst_paise,0), COALESCE(trans_charges_paise,0),
                COALESCE(other_charges_paise,0), COALESCE(total_payable_paise,0)
         FROM import_batches WHERE batch_id = ?1",
        rusqlite::params![batch_id],
        |row| {
            Ok(ImportBatch {
                batch_id:            row.get(0)?,
                account_id:          row.get(1)?,
                source_type:         row.get(2)?,
                file_name:           row.get(3)?,
                ref_no:              row.get(4)?,
                broker:              row.get(5)?,
                batch_trade_date:    row.get(6)?,
                imported_at:         row.get(7)?,
                record_count:        row.get(8)?,
                stt_paise:           row.get(9)?,
                stamp_charges_paise: row.get(10)?,
                gst_paise:           row.get(11)?,
                trans_charges_paise: row.get(12)?,
                other_charges_paise: row.get(13)?,
                total_payable_paise: row.get(14)?,
                transactions:        Vec::new(),
            })
        },
    ).map_err(|e| format!("Batch not found: {e}"))?;

    let mut stmt = conn.prepare(
        "SELECT t.txn_id, t.account_id, a.name, a.portfolio_id,
                COALESCE(t.instrument_id, -t.pending_instrument_id) AS instrument_id,
                COALESCE(i.name, pi.name)                           AS instrument_name,
                ie.isin,
                t.txn_type, t.trade_segment, t.trade_date, t.txn_time,
                t.quantity, t.price_paise, t.brokerage_paise, t.stt_paise,
                t.other_charges_paise, t.total_value_paise,
                t.notes, t.broker_ref,
                t.flag, t.flag_reason, t.flag_dismissed, t.batch_id
         FROM transactions t
         JOIN accounts a ON a.account_id = t.account_id
         LEFT JOIN instruments i ON i.instrument_id = t.instrument_id
         LEFT JOIN pending_instruments pi ON pi.pending_id = t.pending_instrument_id
         LEFT JOIN instrument_equity ie ON ie.instrument_id = t.instrument_id
         WHERE t.batch_id = ?1
         ORDER BY t.trade_date, t.txn_time, t.txn_id",
    ).map_err(|e| e.to_string())?;

    let txns: Vec<Transaction> = stmt
        .query_map(rusqlite::params![batch_id], map_row)
        .map_err(|e| e.to_string())?
        .filter_map(|r| r.ok())
        .collect();

    Ok(ImportBatch { transactions: txns, ..batch })
}
