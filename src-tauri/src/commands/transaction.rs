use crate::db;
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
pub struct GetTransactionsFilter {
    pub account_ids: Option<Vec<i64>>,
    pub instrument_id: Option<i64>,
    pub from_date: Option<String>,
    pub to_date: Option<String>,
    pub txn_type: Option<String>,
    pub limit: Option<i64>,
    pub offset: Option<i64>,
}

#[tauri::command]
pub fn get_transactions(filter: GetTransactionsFilter) -> Result<Vec<Transaction>, String> {
    let conn = db::acquire()?;

    // Build dynamic WHERE clauses
    let mut conditions = vec!["1=1"];
    let mut sql = String::from(
        "SELECT t.txn_id, t.account_id, a.name AS account_name,
                a.portfolio_id,
                t.instrument_id, i.name AS instrument_name, i.isin,
                t.txn_type, t.trade_segment, t.trade_date, t.txn_time,
                t.quantity, t.price_paise, t.brokerage_paise,
                t.stt_paise, t.other_charges_paise, t.total_value_paise,
                t.notes, t.broker_ref
         FROM transactions t
         JOIN accounts a ON t.account_id = a.account_id
         JOIN instruments i ON t.instrument_id = i.instrument_id
         WHERE 1=1"
    );

    if filter.account_ids.as_ref().map(|v| !v.is_empty()).unwrap_or(false) {
        let placeholders = filter.account_ids.as_ref().unwrap()
            .iter().enumerate()
            .map(|(i, _)| format!("?{}", i + 1))
            .collect::<Vec<_>>().join(",");
        sql.push_str(&format!(" AND t.account_id IN ({})", placeholders));
    }
    if filter.from_date.is_some() { sql.push_str(" AND t.trade_date >= ?"); }
    if filter.to_date.is_some()   { sql.push_str(" AND t.trade_date <= ?"); }
    if filter.txn_type.is_some()  { sql.push_str(" AND t.txn_type = ?"); }
    if filter.instrument_id.is_some() { sql.push_str(" AND t.instrument_id = ?"); }

    sql.push_str(" ORDER BY t.trade_date DESC, t.txn_id DESC");
    sql.push_str(&format!(" LIMIT {} OFFSET {}",
        filter.limit.unwrap_or(200),
        filter.offset.unwrap_or(0)
    ));

    // Build params dynamically
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = vec![];
    if let Some(ids) = &filter.account_ids {
        for id in ids { params.push(Box::new(*id)); }
    }
    if let Some(d) = &filter.from_date  { params.push(Box::new(d.clone())); }
    if let Some(d) = &filter.to_date    { params.push(Box::new(d.clone())); }
    if let Some(t) = &filter.txn_type   { params.push(Box::new(t.clone())); }
    if let Some(id) = filter.instrument_id { params.push(Box::new(id)); }

    let _ = conditions; // silence unused warning

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let txns = stmt.query_map(
        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
        |row| {
            Ok(Transaction {
                txn_id: row.get(0)?,
                account_id: row.get(1)?,
                account_name: row.get(2)?,
                portfolio_id: row.get(3)?,
                instrument_id: row.get(4)?,
                instrument_name: row.get(5)?,
                isin: row.get(6)?,
                txn_type: row.get(7)?,
                trade_segment: row.get(8)?,
                trade_date: row.get(9)?,
                txn_time: row.get(10)?,
                quantity: row.get(11)?,
                price_paise: row.get(12)?,
                brokerage_paise: row.get(13)?,
                stt_paise: row.get(14)?,
                other_charges_paise: row.get(15)?,
                total_value_paise: row.get(16)?,
                notes: row.get(17)?,
                broker_ref: row.get(18)?,
            })
        },
    )
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    Ok(txns)
}

#[tauri::command]
pub fn create_transaction(input: CreateTransactionInput) -> Result<Transaction, String> {
    let conn = db::acquire()?;

    // Calculate total value: BUY = -(qty*price + charges), SELL = +(qty*price - charges)
    let gross = (input.quantity * input.price_paise as f64) as i64;
    let charges = input.brokerage_paise + input.stt_paise + input.other_charges_paise;
    let total_value_paise = match input.txn_type.as_str() {
        "BUY" | "SIP"  => -(gross + charges),
        "SELL" | "REDEMPTION" => gross - charges,
        "DIVIDEND" | "INTEREST" => gross,
        _ => gross,
    };

    conn.execute(
        "INSERT INTO transactions
            (account_id, instrument_id, txn_type, trade_segment, trade_date, txn_time,
             quantity, price_paise, brokerage_paise, stt_paise, other_charges_paise,
             total_value_paise, notes, broker_ref)
         VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12,?13,?14)",
        rusqlite::params![
            input.account_id,
            input.instrument_id,
            input.txn_type,
            input.trade_segment,
            input.trade_date,
            input.txn_time,
            input.quantity,
            input.price_paise,
            input.brokerage_paise,
            input.stt_paise,
            input.other_charges_paise,
            total_value_paise,
            input.notes,
            input.broker_ref,
        ],
    ).map_err(|e| e.to_string())?;

    let id = conn.last_insert_rowid();
    get_transaction_by_id(&conn, id).map_err(|e| e.to_string())
}

#[tauri::command]
pub fn delete_transaction(txn_id: i64) -> Result<(), String> {
    let conn = db::acquire()?;
    conn.execute("DELETE FROM transactions WHERE txn_id = ?1", [txn_id])
        .map_err(|e| e.to_string())?;
    Ok(())
}

fn get_transaction_by_id(conn: &rusqlite::Connection, id: i64) -> rusqlite::Result<Transaction> {
    conn.query_row(
        "SELECT t.txn_id, t.account_id, a.name, a.portfolio_id,
                t.instrument_id, i.name, i.isin,
                t.txn_type, t.trade_segment, t.trade_date, t.txn_time,
                t.quantity, t.price_paise, t.brokerage_paise,
                t.stt_paise, t.other_charges_paise, t.total_value_paise,
                t.notes, t.broker_ref
         FROM transactions t
         JOIN accounts a ON t.account_id = a.account_id
         JOIN instruments i ON t.instrument_id = i.instrument_id
         WHERE t.txn_id = ?1",
        [id],
        |row| Ok(Transaction {
            txn_id: row.get(0)?,
            account_id: row.get(1)?,
            account_name: row.get(2)?,
            portfolio_id: row.get(3)?,
            instrument_id: row.get(4)?,
            instrument_name: row.get(5)?,
            isin: row.get(6)?,
            txn_type: row.get(7)?,
            trade_segment: row.get(8)?,
            trade_date: row.get(9)?,
            txn_time: row.get(10)?,
            quantity: row.get(11)?,
            price_paise: row.get(12)?,
            brokerage_paise: row.get(13)?,
            stt_paise: row.get(14)?,
            other_charges_paise: row.get(15)?,
            total_value_paise: row.get(16)?,
            notes: row.get(17)?,
            broker_ref: row.get(18)?,
        }),
    )
}
