pub mod angel_one;
pub mod choice_equity;
pub mod choice_mf;
pub mod icici_equity;
pub mod pdf_utils;

use crate::db;
use serde::Serialize;

#[derive(Serialize)]
pub struct ImportSource {
    pub value:       &'static str,
    pub label:       &'static str,
    pub description: &'static str,
}

#[tauri::command]
pub fn get_import_sources() -> Vec<ImportSource> {
    vec![
        ImportSource { value: "ANGELONE",      label: "Angel One — Trades & Charges",   description: ".xlsx from Angel One back-office"                },
        ImportSource { value: "CHOICE_MF",     label: "Choice Wealth — MF Statement",   description: ".pdf from Choice Wealth MF portal"               },
        ImportSource { value: "CHOICE_EQUITY", label: "Choice Equity — Global Details", description: ".pdf Global Details Report from Choice Equity"   },
        ImportSource { value: "ICICI_EQUITY",  label: "ICICI Securities — Equity TRX",  description: ".pdf TRX-Equity statement from ICICI Securities"  },
    ]
}

/// After importing transactions for an account, run a FIFO simulation
/// per (account, instrument) and flag any SELL/REDEMPTION/TRANSFER_OUT/MERGER_OUT/SWITCH_OUT
/// transactions that exceed the available quantity at that point in time.
///
/// Only unflagged transactions participate in the simulation — already-flagged
/// rows are neither checked nor mutated (so re-running is idempotent for prior flags).
/// Transactions that were previously flagged OVERSELL but now clear (e.g. after the
/// user adds an opening balance) will have their flag cleared automatically.
pub fn flag_oversells(account_id: i64) -> Result<(), String> {
    let conn = db::acquire()?;

    // Load all non-flagged delivery-relevant transactions for this account,
    // ordered to match the FIFO engine in holdings.rs.
    let mut stmt = conn.prepare(
        "SELECT t.txn_id, t.instrument_id, t.trade_date, t.txn_type, t.trade_segment, t.quantity
         FROM transactions t
         WHERE t.account_id = ?1
           AND t.txn_type IN (
               'BUY','SIP','OPENING_BALANCE','BONUS','MERGER_IN','SWITCH_IN','TRANSFER_IN',
               'SELL','REDEMPTION','MERGER_OUT','SWITCH_OUT','TRANSFER_OUT'
           )
           AND (t.flag IS NULL OR t.flag = 'OVERSELL')
         ORDER BY t.instrument_id, t.trade_date ASC, t.txn_id ASC",
    ).map_err(|e| e.to_string())?;

    struct Row { txn_id: i64, instrument_id: i64, txn_type: String, quantity: f64 }

    let rows: Vec<Row> = stmt.query_map([account_id], |row| Ok(Row {
        txn_id:        row.get(0)?,
        instrument_id: row.get(1)?,
        txn_type:      row.get(3)?,
        quantity:      row.get(5)?,
    }))
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    // FIFO per instrument — track running quantity
    let mut qty_map: std::collections::HashMap<i64, f64> = std::collections::HashMap::new();
    let mut to_flag:  Vec<(i64, String)> = Vec::new(); // (txn_id, reason)
    let mut to_clear: Vec<i64>           = Vec::new(); // txn_ids to un-flag

    for row in &rows {
        let qty = qty_map.entry(row.instrument_id).or_insert(0.0);

        let is_sell = matches!(
            row.txn_type.as_str(),
            "SELL" | "REDEMPTION" | "MERGER_OUT" | "SWITCH_OUT" | "TRANSFER_OUT"
        );

        if is_sell {
            if row.quantity > *qty + 0.0001 {
                to_flag.push((
                    row.txn_id,
                    format!("Sell qty {:.4} exceeds available {:.4}", row.quantity, qty),
                ));
                // Don't reduce below zero — the oversell txn is excluded from portfolio
            } else {
                *qty -= row.quantity;
                to_clear.push(row.txn_id);
            }
        } else {
            *qty += row.quantity;
            to_clear.push(row.txn_id);
        }
    }

    // Apply flags and clears in a single transaction
    conn.execute_batch("BEGIN").map_err(|e| e.to_string())?;

    for (txn_id, reason) in &to_flag {
        conn.execute(
            "UPDATE transactions SET flag='OVERSELL', flag_reason=?1, flag_dismissed=0
             WHERE txn_id=?2 AND (flag IS NULL OR flag='OVERSELL')",
            rusqlite::params![reason, txn_id],
        ).map_err(|e| e.to_string())?;

        // If this is a TRANSFER_OUT, also flag the paired TRANSFER_IN.
        // Both legs share the same broker_ref (format: TRF-{ms}).
        conn.execute(
            "UPDATE transactions
             SET flag='PAIRED_OVERSELL',
                 flag_reason='Paired TRANSFER_OUT is oversold — transfer is invalid',
                 flag_dismissed=0
             WHERE broker_ref = (SELECT broker_ref FROM transactions WHERE txn_id = ?1)
               AND txn_type = 'TRANSFER_IN'
               AND broker_ref IS NOT NULL
               AND (flag IS NULL OR flag = 'PAIRED_OVERSELL')",
            [txn_id],
        ).map_err(|e| e.to_string())?;
    }

    // Clear OVERSELL flag from transactions that now have sufficient quantity
    for txn_id in &to_clear {
        conn.execute(
            "UPDATE transactions SET flag=NULL, flag_reason=NULL
             WHERE txn_id=?1 AND flag='OVERSELL'",
            [txn_id],
        ).map_err(|e| e.to_string())?;

        // Clear the paired TRANSFER_IN flag when TRANSFER_OUT is no longer oversold
        conn.execute(
            "UPDATE transactions SET flag=NULL, flag_reason=NULL
             WHERE broker_ref = (SELECT broker_ref FROM transactions WHERE txn_id = ?1)
               AND txn_type = 'TRANSFER_IN'
               AND broker_ref IS NOT NULL
               AND flag = 'PAIRED_OVERSELL'",
            [txn_id],
        ).map_err(|e| e.to_string())?;
    }

    conn.execute_batch("COMMIT").map_err(|e| e.to_string())?;
    Ok(())
}
