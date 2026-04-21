use crate::db;
use crate::commands::import::flag_oversells;
use serde::{Deserialize, Serialize};

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
    pub account_id: i64,
    pub instrument_id: i64,
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
    pub broker_ref: Option<String>,
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
    pub account_ids: Option<Vec<i64>>,
    pub instrument_id: Option<i64>,
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub txn_type: Option<String>,
    /// "all" | "flagged" | "clean"
    pub flag_filter: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[tauri::command]
pub fn get_transactions(filter: GetTransactionsFilter) -> Result<Vec<Transaction>, String> {
    let conn = db::acquire()?;

    let mut sql = String::from(
        "SELECT t.txn_id, t.account_id, a.name AS account_name,
                a.portfolio_id,
                t.instrument_id, i.name AS instrument_name, i.isin,
                t.txn_type, t.trade_segment, t.trade_date, t.txn_time,
                t.quantity, t.price_paise, t.brokerage_paise,
                t.stt_paise, t.other_charges_paise, t.total_value_paise,
                t.notes, t.broker_ref,
                t.flag, t.flag_reason, t.flag_dismissed,
                t.batch_id
         FROM transactions t
         JOIN accounts a ON t.account_id = a.account_id
         JOIN instruments i ON t.instrument_id = i.instrument_id
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
    if filter.instrument_id.is_some(){ sql.push_str(" AND t.instrument_id = ?"); }

    match filter.flag_filter.as_deref() {
        Some("flagged") => sql.push_str(" AND t.flag IS NOT NULL AND t.flag_dismissed = 0"),
        Some("clean")   => sql.push_str(" AND (t.flag IS NULL OR t.flag_dismissed = 1)"),
        _               => {}
    }

    sql.push_str(" ORDER BY t.trade_date DESC, t.txn_id DESC");
    sql.push_str(&format!(" LIMIT {} OFFSET {}",
        filter.limit.unwrap_or(200),
        filter.offset.unwrap_or(0)
    ));

    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];
    if let Some(ids) = &filter.account_ids {
        for id in ids { params.push(Box::new(*id)); }
    }
    if let Some(d) = &filter.from_date   { params.push(Box::new(d.clone())); }
    if let Some(d) = &filter.to_date     { params.push(Box::new(d.clone())); }
    if let Some(t) = &filter.txn_type    { params.push(Box::new(t.clone())); }
    if let Some(id) = filter.instrument_id { params.push(Box::new(id)); }

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

#[tauri::command]
pub fn create_transaction(input: CreateTransactionInput) -> Result<Transaction, String> {
    let conn = db::acquire()?;

    let gross   = (input.quantity * input.price_paise as f64) as i64;
    let charges = input.brokerage_paise + input.stt_paise + input.other_charges_paise;
    let total_value_paise = match input.txn_type.as_str() {
        "BUY" | "SIP" | "OPENING_BALANCE" | "TRANSFER_IN" => -(gross + charges),
        "SELL" | "REDEMPTION" | "TRANSFER_OUT"            =>   gross - charges,
        "DIVIDEND" | "INTEREST"                            =>   gross,
        _                                                  =>   gross,
    };

    conn.execute(
        "INSERT INTO transactions
            (account_id, instrument_id, txn_type, trade_segment, trade_date, txn_time,
             quantity, price_paise, brokerage_paise, stt_paise, other_charges_paise,
             total_value_paise, notes, broker_ref)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        rusqlite::params![
            input.account_id, input.instrument_id, input.txn_type,
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
        "BUY" | "SIP" | "OPENING_BALANCE" | "TRANSFER_IN" => -(gross + charges),
        "SELL" | "REDEMPTION" | "TRANSFER_OUT"            =>   gross - charges,
        "DIVIDEND" | "INTEREST"                            =>   gross,
        _                                                  =>   gross,
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

        conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
        (out_id, in_id)
    };

    flag_oversells(input.from_account_id)?;
    flag_oversells(input.to_account_id)?;

    Ok(TransferResult { transfer_out_txn_id: out_id, transfer_in_txn_id: in_id })
}

fn get_transaction_by_id(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<Transaction> {
    conn.query_row(
        "SELECT t.txn_id, t.account_id, a.name, a.portfolio_id,
                t.instrument_id, i.name, i.isin,
                t.txn_type, t.trade_segment, t.trade_date, t.txn_time,
                t.quantity, t.price_paise, t.brokerage_paise,
                t.stt_paise, t.other_charges_paise, t.total_value_paise,
                t.notes, t.broker_ref,
                t.flag, t.flag_reason, t.flag_dismissed,
                t.batch_id
         FROM transactions t
         JOIN accounts a ON t.account_id = a.account_id
         JOIN instruments i ON t.instrument_id = i.instrument_id
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
                t.instrument_id, i.name, i.isin,
                t.txn_type, t.trade_segment, t.trade_date, t.txn_time,
                t.quantity, t.price_paise, t.brokerage_paise, t.stt_paise,
                t.other_charges_paise, t.total_value_paise,
                t.notes, t.broker_ref,
                t.flag, t.flag_reason, t.flag_dismissed, t.batch_id
         FROM transactions t
         JOIN accounts a ON a.account_id = t.account_id
         JOIN instruments i ON i.instrument_id = t.instrument_id
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
