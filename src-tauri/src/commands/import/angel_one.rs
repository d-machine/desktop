//! Parser for Angel One "Trades And Charges" Excel report (.xlsx).
//!
//! File structure:
//!   - Rows 1-33:  metadata and charges summary (skipped)
//!   - Row 34:     section header "TradeBook And Charges" (skipped)
//!   - Row 35:     column headers
//!   - Row 36+:    trade data — two rows per Order ID:
//!       * Trade row  (Trade ID present): price, qty, STT, exchange, stamp
//!       * Charges row (Trade ID empty):  brokerage, GST
//!
//! Returns a list of ParsedTrade — caller is responsible for matching
//! scrip_name to instrument_id and inserting into transactions.

use crate::db;
use calamine::{open_workbook, Data, Reader, Xlsx};
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// One parsed and merged trade from the Angel One report.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedTrade {
    /// Trade date as YYYY-MM-DD
    pub trade_date: String,
    /// Scrip/contract name as printed (may be truncated, no ISIN)
    pub scrip_name: String,
    /// "BUY" or "SELL"
    pub side: String,
    /// Price per unit in rupees
    pub price_rs: f64,
    /// Number of shares/units/lots
    pub quantity: f64,
    /// "DELIVERY", "INTRADAY", "FNO"
    pub order_type: String,
    /// "CAPITAL" or "FUTURES"
    pub segment: String,
    /// "NSE" or "BSE"
    pub exchange: String,
    /// Angel One Order ID (links trade row + charges row)
    pub order_id: String,
    /// Angel One Trade ID (unique per execution, used as broker_ref)
    pub trade_id: String,
    // Charges in rupees
    pub brokerage_rs: f64,
    pub gst_rs: f64,
    pub stt_rs: f64,
    pub exchange_charges_rs: f64,
    pub stamp_duty_rs: f64,
    pub sebi_tax_rs: f64,
    pub other_charges_rs: f64,
    /// Total charges = brokerage + gst + stt + exchange + stamp + sebi + other
    pub total_charges_rs: f64,
}

/// Result of parsing the full file.
#[derive(Debug, Serialize)]
pub struct AngelOneParseResult {
    pub trades: Vec<ParsedTrade>,
    /// Scrip names that could not be matched to an instrument in our DB.
    pub unmatched_scrips: Vec<String>,
    pub date_range: (String, String),
    pub client_code: String,
}

struct RawRow {
    scrip: String,
    side: String,
    buy_price: f64,
    sell_price: f64,
    qty: f64,
    brokerage: f64,
    gst: f64,
    stt: f64,
    sebi_tax: f64,
    exchange_charges: f64,
    stamp_duty: f64,
    other_charges: f64,
    order_type: String,
    segment: String,
    exchange: String,
    order_id: String,
    trade_id: String,
    trade_date: String,
}

fn cell_str(cell: &Data) -> String {
    match cell {
        Data::String(s) => s.trim().to_string(),
        Data::Float(f) => format!("{}", f),
        Data::Int(i) => format!("{}", i),
        Data::Bool(b) => format!("{}", b),
        _ => String::new(),
    }
}

fn cell_f64(cell: &Data) -> f64 {
    match cell {
        Data::Float(f) => *f,
        Data::Int(i) => *i as f64,
        Data::String(s) => s.trim().parse().unwrap_or(0.0),
        _ => 0.0,
    }
}

fn cell_date(cell: &Data) -> String {
    match cell {
        Data::DateTime(edt) => {
            if let Some(dt) = edt.as_datetime() {
                return dt.format("%Y-%m-%d").to_string();
            }
            String::new()
        }
        Data::DateTimeIso(s) => s.get(..10).unwrap_or("").to_string(),
        Data::String(s) => s.get(..10).unwrap_or("").to_string(),
        _ => String::new(),
    }
}

fn map_order_type(raw: &str, segment: &str) -> String {
    match segment.to_uppercase().as_str() {
        "FUTURES" => "FNO".to_string(),
        _ => match raw.to_uppercase().as_str() {
            "INTRADAY" => "INTRADAY".to_string(),
            _ => "DELIVERY".to_string(),
        },
    }
}

fn get_cell(row: &[Data], idx: usize) -> Data {
    row.get(idx).cloned().unwrap_or(Data::Empty)
}

/// Parse an Angel One TradesAndCharges .xlsx file.
/// Returns parsed trades ready for import preview.
#[tauri::command]
pub fn parse_angel_one_xlsx(file_path: String) -> Result<AngelOneParseResult, String> {
    let mut workbook: Xlsx<_> =
        open_workbook(&file_path).map_err(|e| format!("Failed to open file: {e}"))?;

    let sheet_name = workbook
        .sheet_names()
        .first()
        .ok_or("No sheets in workbook")?
        .clone();

    let range = workbook
        .worksheet_range(&sheet_name)
        .map_err(|e| format!("Failed to read sheet: {e}"))?;

    let rows: Vec<Vec<Data>> = range.rows().map(|r| r.to_vec()).collect();

    // --- Extract metadata from header section ---
    let mut client_code = String::new();
    let mut start_date = String::new();
    let mut end_date = String::new();

    for row in rows.iter().take(10) {
        if row.is_empty() {
            continue;
        }
        let label = cell_str(&row[0]);
        match label.as_str() {
            "ClientCode" => client_code = cell_str(row.get(1).unwrap_or(&Data::Empty)),
            "StartDate" => {
                start_date = cell_str(row.get(1).unwrap_or(&Data::Empty))
                    .get(..10)
                    .unwrap_or("")
                    .to_string()
            }
            "EndDate" => {
                end_date = cell_str(row.get(1).unwrap_or(&Data::Empty))
                    .get(..10)
                    .unwrap_or("")
                    .to_string()
            }
            _ => {}
        }
    }

    // --- Find header row (contains "Scrip/Contract") ---
    let header_row_idx = rows
        .iter()
        .position(|row| {
            row.first()
                .map(|c| cell_str(c) == "Scrip/Contract")
                .unwrap_or(false)
        })
        .ok_or("Could not find trade data header row")?;

    // Columns (0-based):
    // 0:Scrip  1:Buy/Sell  2:BuyPrice  3:SellPrice  4:Qty
    // 5:Brokerage  6:GST  7:STT  8:SebiTax  9:ExchangeCharges
    // 10:StampDuty  11:OtherCharges  12:IPFTCharges
    // 13:OrderType  14:Segment  15:Exchange  16:OrderID  17:TradeID  18:Date

    let mut raw_rows: Vec<RawRow> = Vec::new();

    for row in rows.iter().skip(header_row_idx + 1) {
        if row.iter().all(|c| matches!(c, Data::Empty)) {
            continue;
        }
        let scrip = cell_str(&get_cell(row, 0));
        if scrip.is_empty() {
            continue;
        }

        raw_rows.push(RawRow {
            scrip,
            side: cell_str(&get_cell(row, 1)),
            buy_price: cell_f64(&get_cell(row, 2)),
            sell_price: cell_f64(&get_cell(row, 3)),
            qty: cell_f64(&get_cell(row, 4)),
            brokerage: cell_f64(&get_cell(row, 5)),
            gst: cell_f64(&get_cell(row, 6)),
            stt: cell_f64(&get_cell(row, 7)),
            sebi_tax: cell_f64(&get_cell(row, 8)),
            exchange_charges: cell_f64(&get_cell(row, 9)),
            stamp_duty: cell_f64(&get_cell(row, 10)),
            other_charges: cell_f64(&get_cell(row, 11)) + cell_f64(&get_cell(row, 12)), // Other + IPFT
            order_type: cell_str(&get_cell(row, 13)),
            segment: cell_str(&get_cell(row, 14)),
            exchange: cell_str(&get_cell(row, 15)),
            order_id: cell_str(&get_cell(row, 16)),
            trade_id: cell_str(&get_cell(row, 17)),
            trade_date: cell_date(&get_cell(row, 18)),
        });
    }

    // Build brokerage/GST lookup from charge rows (Trade ID empty), keyed by Order ID
    let mut charge_map: HashMap<String, (f64, f64)> = HashMap::new();
    for r in raw_rows.iter().filter(|r| r.trade_id.is_empty()) {
        let entry = charge_map.entry(r.order_id.clone()).or_insert((0.0, 0.0));
        entry.0 += r.brokerage;
        entry.1 += r.gst;
    }

    // Process trade rows (Trade ID present), merge with charges
    let mut trades: Vec<ParsedTrade> = Vec::new();
    let mut unmatched_scrips: std::collections::HashSet<String> =
        std::collections::HashSet::new();

    for r in raw_rows.iter().filter(|r| !r.trade_id.is_empty()) {
        let side_upper = r.side.to_uppercase();
        let price = if side_upper == "BUY" {
            r.buy_price
        } else {
            r.sell_price
        };
        let (brokerage, gst) = charge_map.get(&r.order_id).copied().unwrap_or((0.0, 0.0));
        let total_charges = brokerage
            + gst
            + r.stt
            + r.exchange_charges
            + r.stamp_duty
            + r.sebi_tax
            + r.other_charges;
        let order_type = map_order_type(&r.order_type, &r.segment);

        unmatched_scrips.insert(r.scrip.clone());

        trades.push(ParsedTrade {
            trade_date: r.trade_date.clone(),
            scrip_name: r.scrip.clone(),
            side: side_upper,
            price_rs: price,
            quantity: r.qty,
            order_type,
            segment: r.segment.clone(),
            exchange: r.exchange.clone(),
            order_id: r.order_id.clone(),
            trade_id: r.trade_id.clone(),
            brokerage_rs: brokerage,
            gst_rs: gst,
            stt_rs: r.stt,
            exchange_charges_rs: r.exchange_charges,
            stamp_duty_rs: r.stamp_duty,
            sebi_tax_rs: r.sebi_tax,
            other_charges_rs: r.other_charges,
            total_charges_rs: total_charges,
        });
    }

    // Sort by date ascending
    trades.sort_by(|a, b| a.trade_date.cmp(&b.trade_date));

    let unmatched: Vec<String> = unmatched_scrips.into_iter().collect();

    Ok(AngelOneParseResult {
        trades,
        unmatched_scrips: unmatched,
        date_range: (start_date, end_date),
        client_code,
    })
}

#[derive(Debug, Serialize)]
pub struct AngelOneImportResult {
    pub imported: usize,
    pub skipped: usize,
    /// Scrips that were auto-created as placeholder instruments (no ISIN yet).
    pub auto_created_instruments: Vec<String>,
}

/// Bulk-insert parsed Angel One trades into the DB for a given account.
///
/// Instrument lookup order:
///   1. Match `instrument_equity.nse_symbol` (exact, case-insensitive)
///   2. Match `instruments.name` (exact, case-insensitive)
///   3. Auto-create a placeholder equity instrument using the scrip name as NSE symbol.
///      These can be enriched with ISIN later via the server price sync.
///
/// Skips trades whose broker_ref (Trade ID) already exists for this account.
#[tauri::command]
pub fn import_angel_one_trades(
    account_id: i64,
    trades: Vec<ParsedTrade>,
) -> Result<AngelOneImportResult, String> {
    let conn = db::acquire()?;
    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut auto_created: Vec<String> = Vec::new();

    // instrument_type_id for EQUITY (1 per migration seed)
    let equity_type_id: i64 = conn
        .query_row(
            "SELECT instrument_type_id FROM instrument_types WHERE name='EQUITY' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(1);

    // exchange_id for NSE
    let nse_exchange_id: Option<i64> = conn
        .query_row(
            "SELECT exchange_id FROM exchanges WHERE code='NSE' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    for trade in &trades {
        let scrip_upper = trade.scrip_name.to_uppercase();

        // 1. Try NSE symbol match
        let mut instrument_id: Option<i64> = conn
            .query_row(
                "SELECT ie.instrument_id FROM instrument_equity ie
                 WHERE UPPER(ie.nse_symbol) = ?1 LIMIT 1",
                [&scrip_upper],
                |row| row.get(0),
            )
            .ok();

        // 2. Try instrument name match
        if instrument_id.is_none() {
            instrument_id = conn
                .query_row(
                    "SELECT instrument_id FROM instruments WHERE UPPER(name) = ?1 LIMIT 1",
                    [&scrip_upper],
                    |row| row.get(0),
                )
                .ok();
        }

        // 3. Auto-create placeholder equity instrument
        if instrument_id.is_none() {
            conn.execute(
                "INSERT OR IGNORE INTO instruments (name, instrument_type_id, primary_exchange_id, source)
                 VALUES (?1, ?2, ?3, 'IMPORT')",
                rusqlite::params![trade.scrip_name, equity_type_id, nse_exchange_id],
            )
            .map_err(|e| e.to_string())?;

            let new_id: i64 = conn
                .query_row(
                    "SELECT instrument_id FROM instruments WHERE name = ?1 ORDER BY instrument_id DESC LIMIT 1",
                    [&trade.scrip_name],
                    |row| row.get(0),
                )
                .map_err(|e| e.to_string())?;

            // Also insert into instrument_equity so future NSE symbol lookups work
            conn.execute(
                "INSERT OR IGNORE INTO instrument_equity (instrument_id, nse_symbol) VALUES (?1, ?2)",
                rusqlite::params![new_id, scrip_upper],
            )
            .map_err(|e| e.to_string())?;

            instrument_id = Some(new_id);
            if !auto_created.contains(&trade.scrip_name) {
                auto_created.push(trade.scrip_name.clone());
            }
        }

        let instrument_id = instrument_id.unwrap();

        // Deduplicate by (account_id, broker_ref)
        if !trade.trade_id.is_empty() {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM transactions WHERE account_id=?1 AND broker_ref=?2",
                    rusqlite::params![account_id, &trade.trade_id],
                    |row| row.get::<_, i64>(0),
                )
                .map(|c| c > 0)
                .unwrap_or(false);
            if exists {
                skipped += 1;
                continue;
            }
        }

        let price_paise = (trade.price_rs * 100.0).round() as i64;
        let brokerage_paise = (trade.brokerage_rs * 100.0).round() as i64;
        let stt_paise = (trade.stt_rs * 100.0).round() as i64;
        let other_paise = ((trade.gst_rs
            + trade.exchange_charges_rs
            + trade.stamp_duty_rs
            + trade.sebi_tax_rs
            + trade.other_charges_rs)
            * 100.0)
            .round() as i64;
        let gross = (trade.quantity * trade.price_rs * 100.0).round() as i64;
        let charges = brokerage_paise + stt_paise + other_paise;
        let total_value_paise = if trade.side == "BUY" {
            -(gross + charges)
        } else {
            gross - charges
        };
        let segment = match trade.order_type.as_str() {
            "FNO" => "DERIVATIVES",
            "INTRADAY" => "INTRADAY",
            _ => "EQUITY",
        };
        let broker_ref: Option<&str> = if trade.trade_id.is_empty() {
            None
        } else {
            Some(&trade.trade_id)
        };

        conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, txn_type, trade_segment, trade_date,
                 quantity, price_paise, brokerage_paise, stt_paise, other_charges_paise,
                 total_value_paise, broker_ref)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,?9,?10,?11,?12)",
            rusqlite::params![
                account_id,
                instrument_id,
                trade.side,
                segment,
                trade.trade_date,
                trade.quantity,
                price_paise,
                brokerage_paise,
                stt_paise,
                other_paise,
                total_value_paise,
                broker_ref,
            ],
        )
        .map_err(|e| e.to_string())?;

        imported += 1;
    }

    Ok(AngelOneImportResult {
        imported,
        skipped,
        auto_created_instruments: auto_created,
    })
}
