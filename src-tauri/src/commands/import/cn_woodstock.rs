//! Parser for Woodstock Broking — Contract Note PDF.
//!
//! All trade data lives in the summary table on CN pages (pages that do NOT
//! contain "Detail Trade Annexure").  If the table overflows, the header repeats
//! on the next page — those rows are automatically skipped because they don't
//! match the ISIN pattern.
//!
//! Per-page layout:
//!   - Page 1 : Metadata (CN number, trade date, client code) + trade table start + charges
//!   - Overflow pages : repeat table header + more ISIN rows; charges on the last CN page
//!   - Annexure pages : "Detail Trade Annexure" header detected → skipped entirely
//!
//! BUY/SELL is determined by which half of the table row contains data.
//! All trades are imported as DELIVERY — intraday is a reporting-time concept.

use crate::{commands::import::{common, pdf_utils, flag_oversells}, db};
use crate::commands::import::cn_choice_equity::ParsedCharge;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;
use regex::Regex;

// ─── Constants ────────────────────────────────────────────────────────────────

const X_GAP:      f32 = 6.0;
const CHAR_Y_TOL: f32 = 3.5;
const ROW_Y_TOL:  f32 = 3.5;

// Column X boundaries derived from actual PDF span positions.
const X_ISIN_MAX:     f32 = 90.0;   // ISIN:           x <  90
const X_SCRIP_MIN:    f32 = 90.0;   // Scrip name:     90 – 225
const X_SCRIP_MAX:    f32 = 225.0;
const X_BUY_QTY_MIN:  f32 = 225.0;  // BUY  qty:      225 – 265
const X_BUY_QTY_MAX:  f32 = 265.0;
const X_BUY_WAP_MIN:  f32 = 265.0;  // BUY  WAP Mkt:  265 – 308  (Brok Rate starts at ~312)
const X_BUY_WAP_MAX:  f32 = 308.0;
const X_SELL_QTY_MIN: f32 = 445.0;  // SELL qty:      445 – 490
const X_SELL_QTY_MAX: f32 = 490.0;
const X_SELL_WAP_MIN: f32 = 488.0;  // SELL WAP Mkt:  488 – 532  (Brok Rate starts at ~540)
const X_SELL_WAP_MAX: f32 = 532.0;
const X_CHARGE_AMT:   f32 = 740.0;  // Charge amounts: x > 740

// ─── Regex ────────────────────────────────────────────────────────────────────

fn isin_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^IN[A-Z0-9]{10}$").unwrap())
}

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedWoodstockTrade {
    pub isin:       String,
    pub scrip_name: String,
    pub buy_sell:   String,  // "BUY" | "SELL"
    pub quantity:   f64,
    pub price:      f64,     // WAP Mkt Rate in rupees
}

#[derive(Debug, Serialize)]
pub struct WoodstockCnParseResult {
    pub trade_date:    String,
    pub cn_number:     String,
    pub client_code:   Option<String>,
    pub trades:        Vec<ParsedWoodstockTrade>,
    pub charges:       Vec<ParsedCharge>,
    pub pages_scanned: usize,
}

#[derive(Debug, Serialize)]
pub struct WoodstockCnImportResult {
    pub imported:                 usize,
    pub skipped:                  usize,
    pub auto_created_instruments: usize,
}

// ─── Helpers ──────────────────────────────────────────────────────────────────

fn month_num(abbr: &str) -> Option<u32> {
    match abbr.to_lowercase().as_str() {
        "jan" => Some(1),  "feb" => Some(2),  "mar" => Some(3),
        "apr" => Some(4),  "may" => Some(5),  "jun" => Some(6),
        "jul" => Some(7),  "aug" => Some(8),  "sep" => Some(9),
        "oct" => Some(10), "nov" => Some(11), "dec" => Some(12),
        _ => None,
    }
}

// "17 Sep 2025" → "2025-09-17"
fn parse_woodstock_date(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.trim().split_whitespace().collect();
    if parts.len() < 3 { return None; }
    let day: u32   = parts[0].parse().ok()?;
    let month      = month_num(parts[1])?;
    let year: u32  = parts[2].parse().ok()?;
    Some(format!("{year}-{month:02}-{day:02}"))
}

fn parse_num(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() || s == "-" { return 0.0; }
    s.replace(',', "").parse().unwrap_or(0.0)
}

/// Concatenate all span texts in the X range [x_min, x_max).
fn spans_in_x(row: &[pdf_utils::TextSpan], x_min: f32, x_max: f32) -> String {
    row.iter()
        .filter(|s| s.x >= x_min && s.x < x_max)
        .map(|s| s.text.trim())
        .filter(|s| !s.is_empty())
        .collect::<Vec<_>>()
        .join("")
}

// ─── Page classification ──────────────────────────────────────────────────────

fn is_annexure_page(rows: &[Vec<pdf_utils::TextSpan>]) -> bool {
    rows.iter().any(|row| {
        row.iter().any(|s| s.text.contains("Detail Trade Annexure"))
    })
}

// ─── Metadata ─────────────────────────────────────────────────────────────────

fn extract_metadata(
    rows: &[Vec<pdf_utils::TextSpan>],
) -> (String, String, Option<String>) {
    let mut cn_number  = String::new();
    let mut trade_date = String::new();
    let mut client_code: Option<String> = None;

    for row in rows {
        // CN number: label span at x > 600 containing "Contract No"
        // followed by value span ": 639"
        if cn_number.is_empty() {
            if let Some(idx) = row.iter().position(|s| s.x > 600.0 && s.text.contains("Contract No")) {
                if let Some(val) = row.get(idx + 1) {
                    let v = val.text.trim().trim_start_matches(':').trim().to_string();
                    if !v.is_empty() { cn_number = v; }
                }
            }
        }

        // Trade date: label span at x > 600 containing "Trade Date"
        // followed by value span ": 17 Sep 2025"
        if trade_date.is_empty() {
            if let Some(idx) = row.iter().position(|s| s.x > 600.0 && s.text.contains("Trade Date")) {
                if let Some(val) = row.get(idx + 1) {
                    let raw = val.text.trim().trim_start_matches(':').trim().to_string();
                    if let Some(iso) = parse_woodstock_date(&raw) {
                        trade_date = iso;
                    }
                }
            }
        }

        // Client code: row with "BackOffice Code", value in span at x > 100
        // Format: [25|Trading/ BackOffice Code]  [131|: S047]
        if client_code.is_none() {
            let has_label = row.iter().any(|s| s.text.contains("BackOffice Code"));
            if has_label {
                if let Some(val) = row.iter().find(|s| s.x > 100.0 && s.text.contains(':')) {
                    let v = val.text.trim().trim_start_matches(':').trim().to_string();
                    if !v.is_empty() { client_code = Some(v); }
                }
            }
        }
    }

    (cn_number, trade_date, client_code)
}

// ─── Trade table ──────────────────────────────────────────────────────────────

fn extract_trades(rows: &[Vec<pdf_utils::TextSpan>]) -> Vec<ParsedWoodstockTrade> {
    let mut trades = Vec::new();

    for row in rows {
        // Data row: first span must be an ISIN (x < 90, matches IN + 10 alphanum)
        let first = match row.first() {
            Some(s) if s.x < X_ISIN_MAX => s,
            _ => continue,
        };
        let isin = first.text.trim().to_string();
        if !isin_re().is_match(&isin) { continue; }

        let scrip = spans_in_x(row, X_SCRIP_MIN, X_SCRIP_MAX).trim().to_string();

        // BUY side
        let buy_qty = parse_num(&spans_in_x(row, X_BUY_QTY_MIN, X_BUY_QTY_MAX));
        if buy_qty > 0.0 {
            let price = parse_num(&spans_in_x(row, X_BUY_WAP_MIN, X_BUY_WAP_MAX));
            trades.push(ParsedWoodstockTrade {
                isin: isin.clone(),
                scrip_name: scrip.clone(),
                buy_sell: "BUY".to_string(),
                quantity: buy_qty,
                price,
            });
        }

        // SELL side
        let sell_qty = parse_num(&spans_in_x(row, X_SELL_QTY_MIN, X_SELL_QTY_MAX));
        if sell_qty > 0.0 {
            let price = parse_num(&spans_in_x(row, X_SELL_WAP_MIN, X_SELL_WAP_MAX));
            trades.push(ParsedWoodstockTrade {
                isin: isin.clone(),
                scrip_name: scrip.clone(),
                buy_sell: "SELL".to_string(),
                quantity: sell_qty,
                price,
            });
        }
    }

    trades
}

// ─── Charges ──────────────────────────────────────────────────────────────────

fn extract_charges(rows: &[Vec<pdf_utils::TextSpan>]) -> Vec<ParsedCharge> {
    let mut accum: std::collections::HashMap<String, i64> = Default::default();

    for row in rows {
        let row_text: String = row.iter()
            .map(|s| s.text.trim())
            .filter(|s| !s.is_empty())
            .collect::<Vec<_>>()
            .join(" ");
        let lc = row_text.to_lowercase();

        let key = if lc.contains("brokerage") {
            Some("BROKERAGE")
        } else if lc.contains("cgst") || lc.contains("sgst") {
            Some("GST")
        } else if lc.contains("security trx tax") || lc.contains("securities transaction") {
            Some("STT")
        } else if lc.contains("stamp duty") {
            Some("STAMP_DUTY")
        } else if lc.contains("transaction charges") {
            Some("TRANSACTION_CHARGES")
        } else if lc.contains("sebi fees") || lc.contains("sebi turnover") {
            Some("SEBI_TURNOVER_FEES")
        } else {
            None
        };

        if let Some(k) = key {
            // Amount is the rightmost span with x > X_CHARGE_AMT
            if let Some(span) = row.iter()
                .filter(|s| s.x > X_CHARGE_AMT)
                .max_by(|a, b| a.x.partial_cmp(&b.x).unwrap_or(std::cmp::Ordering::Equal))
            {
                let v: f64 = span.text.trim().replace(',', "").parse().unwrap_or(0.0);
                if v > 0.0 {
                    *accum.entry(k.to_string()).or_insert(0) += (v * 100.0).round() as i64;
                }
            }
        }
    }

    accum.into_iter()
        .filter(|(_, p)| *p > 0)
        .map(|(ct, p)| ParsedCharge { charge_type: ct, amount_paise: p })
        .collect()
}

// ─── Parse command ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn parse_cn_woodstock_pdf(
    file_path: String,
    password:  Option<String>,
) -> Result<WoodstockCnParseResult, String> {
    let doc = pdf_utils::load_pdf(&file_path, password.as_deref())?;
    let all_page_spans = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], X_GAP, CHAR_Y_TOL)?;

    let pages_scanned = all_page_spans.len();
    let mut all_cn_rows:   Vec<Vec<pdf_utils::TextSpan>> = Vec::new();
    let mut first_page_rows: Option<Vec<Vec<pdf_utils::TextSpan>>> = None;

    for page_spans in &all_page_spans {
        let rows = pdf_utils::page_spans_to_rows(page_spans.clone(), ROW_Y_TOL);
        if is_annexure_page(&rows) { continue; }
        if first_page_rows.is_none() {
            first_page_rows = Some(rows.clone());
        }
        all_cn_rows.extend(rows);
    }

    let page1_rows = first_page_rows.unwrap_or_default();
    let (cn_number, trade_date, client_code) = extract_metadata(&page1_rows);
    let trades  = extract_trades(&all_cn_rows);
    let charges = extract_charges(&all_cn_rows);

    Ok(WoodstockCnParseResult {
        trade_date,
        cn_number,
        client_code,
        trades,
        charges,
        pages_scanned,
    })
}

// ─── Import command ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn import_cn_woodstock_trades(
    app:        tauri::AppHandle,
    account_id: i64,
    trade_date: String,
    cn_number:  String,
    trades:     Vec<ParsedWoodstockTrade>,
    charges:    Vec<ParsedCharge>,
    file_paths: Option<Vec<String>>,
) -> Result<WoodstockCnImportResult, String> {
    let conn = db::acquire()?;
    let mut imported     = 0usize;
    let mut skipped      = 0usize;
    let mut auto_created = 0usize;

    let ref_no = if cn_number.is_empty() || cn_number == "0" {
        file_paths.as_ref()
            .and_then(|ps| ps.first())
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("CN-WS-{}", chrono::Utc::now().timestamp()))
    } else {
        format!("CN-WS-{cn_number}")
    };

    let existing_batch_id: Option<i64> = conn.query_row(
        "SELECT batch_id FROM import_batches
         WHERE account_id=?1 AND source_type='CN_WOODSTOCK' AND ref_no=?2",
        rusqlite::params![account_id, ref_no],
        |r| r.get(0),
    ).ok();
    let newly_created = existing_batch_id.is_none();

    let batch_id = if let Some(id) = existing_batch_id {
        id
    } else {
        conn.execute(
            "INSERT INTO import_batches
                (account_id, source_type, ref_no, broker, batch_trade_date)
             VALUES (?1, 'CN_WOODSTOCK', ?2, 'Woodstock Broking', ?3)",
            rusqlite::params![account_id, ref_no, trade_date],
        ).map_err(|e| e.to_string())?;
        conn.last_insert_rowid()
    };

    for trade in &trades {
        let (instrument_id, pending_instrument_id) = match common::resolve_equity(
            &conn,
            &trade.scrip_name,
            Some(&trade.isin),
            None,
            None,
            None,
            &mut auto_created,
        ) {
            Some(pair) => pair,
            None => { skipped += 1; continue; }
        };

        let broker_ref  = format!("{}-{}-{}", ref_no, trade.isin, trade.buy_sell);
        let price_paise = (trade.price * 100.0).round() as i64;
        let gross_paise = (trade.quantity * trade.price * 100.0).round() as i64;
        let total_value = if trade.buy_sell == "BUY" { -gross_paise } else { gross_paise };

        let rows = conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, pending_instrument_id, txn_type, trade_segment,
                 trade_date, quantity, price_paise, brokerage_paise, stt_paise,
                 other_charges_paise, total_value_paise, notes, broker_ref, batch_id)
             VALUES (?1,?2,?3,?4,'DELIVERY',?5,?6,?7,0,0,0,?8,NULL,?9,?10)
             ON CONFLICT(account_id, broker_ref) WHERE broker_ref IS NOT NULL DO UPDATE SET
                 instrument_id         = excluded.instrument_id,
                 pending_instrument_id = excluded.pending_instrument_id,
                 batch_id              = excluded.batch_id
             WHERE transactions.instrument_id         IS NOT excluded.instrument_id
                OR transactions.pending_instrument_id IS NOT excluded.pending_instrument_id",
            rusqlite::params![
                account_id, instrument_id, pending_instrument_id,
                trade.buy_sell, trade_date,
                trade.quantity, price_paise, total_value,
                broker_ref, batch_id,
            ],
        ).map_err(|e| e.to_string())?;

        if rows > 0 { imported += 1; } else { skipped += 1; }
    }

    if newly_created && !charges.is_empty() && !trade_date.is_empty() {
        for charge in &charges {
            if charge.amount_paise <= 0 { continue; }
            let _ = conn.execute(
                "INSERT INTO charges
                    (account_id, start_date, end_date, charge_type, amount_paise, source, import_batch_id)
                 VALUES (?1, ?2, ?3, ?4, ?5, 'IMPORT', ?6)",
                rusqlite::params![
                    account_id, trade_date, trade_date,
                    charge.charge_type, charge.amount_paise, batch_id,
                ],
            );
        }
    }

    if imported > 0 {
        if let Some(paths) = file_paths {
            if let Some(name_json) = common::copy_statements(&app, paths) {
                common::update_batch_file_names(&conn, &[batch_id], &name_json);
            }
        }
    }

    drop(conn);
    flag_oversells(account_id)?;

    Ok(WoodstockCnImportResult { imported, skipped, auto_created_instruments: auto_created })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const WOODSTOCK_PDF: &str =
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\17.9.2025-WOODSTOCK.pdf";

    fn skip_if_missing(path: &str) -> bool {
        if !std::path::Path::new(path).exists() {
            println!("SKIP: {path} not found");
            true
        } else {
            false
        }
    }

    #[test]
    fn test_parse_woodstock_cn() {
        if skip_if_missing(WOODSTOCK_PDF) { return; }
        let result = parse_cn_woodstock_pdf(WOODSTOCK_PDF.to_string(), None)
            .expect("parse failed");
        println!("Trade date: {}  CN#: {}  client: {:?}  pages: {}",
            result.trade_date, result.cn_number, result.client_code, result.pages_scanned);
        println!("Trades: {}", result.trades.len());
        for t in &result.trades {
            println!("  {:4} {}  qty={:>8.0}  @ {:>10.4}  {}",
                t.buy_sell, t.isin, t.quantity, t.price, t.scrip_name);
        }
        println!("Charges:");
        for c in &result.charges {
            println!("  {:25} = {:>10.2}", c.charge_type, c.amount_paise as f64 / 100.0);
        }
        assert!(!result.trades.is_empty(),    "expected trades");
        assert!(!result.trade_date.is_empty(), "expected trade date");
        assert!(!result.cn_number.is_empty(),  "expected CN number");
    }

    /// Diagnostic: dump page structure to verify column positions still hold.
    #[test]
    fn test_dump_page1_rows() {
        if skip_if_missing(WOODSTOCK_PDF) { return; }
        let doc   = lopdf::Document::load(WOODSTOCK_PDF).expect("load PDF");
        let spans = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], 6.0, 3.5)
            .expect("extract spans");
        let page1 = match spans.first() {
            Some(s) => s,
            None => { println!("No page 1"); return; }
        };
        let rows = pdf_utils::page_spans_to_rows(page1.clone(), 3.5);
        println!("\n=== Page 1 — {} rows ===", rows.len());
        for (ri, row) in rows.iter().enumerate() {
            let cells: Vec<String> = row.iter()
                .map(|s| format!("[{:.0}|{}]", s.x, s.text.trim()))
                .collect();
            println!("  row{ri:>3}: {}", cells.join("  "));
        }
    }
}
