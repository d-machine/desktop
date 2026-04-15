//! Parser for Choice Wealth PVT LTD MF Transaction Report PDF.
//!
//! Pure Rust implementation using pdf-extract (backed by lopdf).
//! No native library or Python required.
//!
//! PDF layout (page size ≈ 1000 × 700 pt):
//!   Col 0 (x≈22):  Transaction Date   (dd/mm/yyyy)
//!   Col 1 (x≈165): Transaction Type
//!   Col 2 (x≈305): Security/Scheme Name + Folio/ISIN (continuation line)
//!   Col 3 (x≈610): Amount (Rs.)
//!   Col 4 (x≈715): Nav/Price (Rs.)
//!   Col 5 (x≈800): Units/Quantity
//!   Col 6 (x≈895): Current Value (Rs.)  — parsed but not stored

use crate::{commands::import::pdf_utils, db};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// Column left-edge boundaries (PDF X coordinates, same as pdfplumber's x).
// Defined slightly left of each header's left edge so right-aligned numbers
// (which start a few points to the right of the header) are captured correctly.
const MF_COL_X: [f32; 7] = [22.0, 165.0, 305.0, 610.0, 715.0, 800.0, 895.0];

fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{1,2}/\d{1,2}/\d{4}$").unwrap())
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedMfTransaction {
    pub trade_date: String,
    pub scheme_name: String,
    pub isin: String,
    pub folio: String,
    /// Normalised: "SIP", "BUY", "REDEMPTION", "SWITCH_IN", "SWITCH_OUT", "DIVIDEND"
    pub txn_type: String,
    pub amount_rs: f64,
    pub nav_rs: f64,
    pub units: f64,
    /// Original type string from the report
    pub raw_type: String,
}

#[derive(Debug, Serialize)]
pub struct ChoiceMfParseResult {
    pub transactions: Vec<ParsedMfTransaction>,
    pub total_rows: usize,
    pub skipped_rows: usize,
}

fn classify_txn_type(raw: &str) -> &'static str {
    let t = raw.to_uppercase();
    if t.contains("FRESH") || t.contains("NEW") {
        "BUY"
    } else if t.contains("ADDITIONAL") || t.contains("SYSTEMATIC") || t.contains("SIP") {
        "SIP"
    } else if t.contains("REDEMPTION") || t.contains("REDEEM") {
        "REDEMPTION"
    } else if t.contains("SWITCH IN") || t.contains("SWITCH-IN") {
        "SWITCH_IN"
    } else if t.contains("SWITCH OUT") || t.contains("SWITCH-OUT") {
        "SWITCH_OUT"
    } else if t.contains("DIVIDEND") || t.contains("IDCW") {
        "DIVIDEND"
    } else {
        "BUY"
    }
}

fn parse_amount(s: &str) -> f64 {
    s.replace(',', "").trim().parse().unwrap_or(0.0)
}

fn parse_date(s: &str) -> String {
    // "30/12/2024" → "2024-12-30"
    let parts: Vec<&str> = s.trim().splitn(3, '/').collect();
    if parts.len() == 3 && parts[2].len() == 4 {
        return format!("{}-{:0>2}-{:0>2}", parts[2], parts[1], parts[0]);
    }
    s.trim().to_string()
}

/// Split "Scheme Name\nFolioNo/.../.../ISIN" into (name, folio, isin).
fn extract_isin_folio(scheme_field: &str) -> (String, String, String) {
    let lines: Vec<&str> = scheme_field
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return (String::new(), String::new(), String::new());
    }
    let scheme_name = lines[0].to_string();
    if lines.len() < 2 {
        return (scheme_name, String::new(), String::new());
    }

    let folio_isin = lines.last().unwrap();
    let parts: Vec<&str> = folio_isin.split('/').collect();

    // ISIN: last segment matching IN* pattern, 12 chars
    let mut isin = String::new();
    for part in parts.iter().rev() {
        let p = part.trim();
        if (p.starts_with("INF") || p.starts_with("IN")) && p.len() == 12 {
            isin = p.to_string();
            break;
        }
    }

    let folio = parts
        .iter()
        .filter(|p| p.trim() != isin.as_str())
        .cloned()
        .collect::<Vec<_>>()
        .join("/");

    (scheme_name, folio, isin)
}

/// Parse a Choice Wealth MF Transaction Report PDF.
/// Returns the parsed transaction rows. No account selection is needed here.
#[tauri::command]
pub fn parse_choice_mf_pdf(file_path: String) -> Result<ChoiceMfParseResult, String> {
    let all_pages = pdf_utils::extract_all_page_spans(&file_path)?;

    let mut transactions: Vec<ParsedMfTransaction> = Vec::new();
    let mut skipped = 0usize;

    // State for the current in-progress transaction (used when a row spans
    // multiple physical lines: main line has date+amounts, continuation has
    // the rest of the type string and the Folio/ISIN).
    let mut pending: Option<ParsedMfTransaction> = None;

    for page_spans in all_pages {
        let rows = pdf_utils::page_spans_to_rows(page_spans, 5.0);

        for row in rows {
            if row.is_empty() {
                continue;
            }

            let mut cells = pdf_utils::spans_to_cells(&row, &MF_COL_X);
            cells.resize(7, String::new());

            let date_str = cells[0].trim().to_string();
            let type_str = cells[1].trim().to_string();
            let scheme_str = cells[2].trim().to_string();
            let amount_str = cells[3].trim().to_string();
            let nav_str = cells[4].trim().to_string();
            let units_str = cells[5].trim().to_string();

            // Skip header rows
            if date_str.to_lowercase().contains("date")
                || type_str.to_lowercase().contains("type")
            {
                continue;
            }
            // Skip summary / total rows (last page summary table)
            if date_str.to_lowercase().contains("transaction") {
                continue;
            }

            let is_date = date_re().is_match(date_str.trim());
            let amount = parse_amount(&amount_str);

            if is_date && amount > 0.0 {
                // Commit previous pending transaction
                if let Some(t) = pending.take() {
                    transactions.push(t);
                }
                // Build this transaction immediately
                let (scheme_name, folio, isin) = extract_isin_folio(&scheme_str);
                pending = Some(ParsedMfTransaction {
                    trade_date: parse_date(&date_str),
                    scheme_name,
                    isin,
                    folio,
                    txn_type: classify_txn_type(&type_str).to_string(),
                    amount_rs: amount,
                    nav_rs: parse_amount(&nav_str),
                    units: parse_amount(&units_str),
                    raw_type: type_str,
                });
            } else if is_date && amount == 0.0 {
                // Date present but no amount yet — shouldn't happen in this PDF
                // but handle gracefully
                skipped += 1;
            } else if !is_date {
                // Continuation row — append folio/ISIN / extra type text
                if let Some(ref mut t) = pending {
                    // Append type continuation
                    if !type_str.is_empty() {
                        if !t.raw_type.is_empty() {
                            t.raw_type.push(' ');
                        }
                        t.raw_type.push_str(&type_str);
                        t.txn_type = classify_txn_type(&t.raw_type).to_string();
                    }
                    // Append scheme/folio/ISIN continuation
                    if !scheme_str.is_empty() && (t.isin.is_empty() || t.folio.is_empty()) {
                        let combined = format!("{}\n{}", t.scheme_name, scheme_str);
                        let (name, folio, isin) = extract_isin_folio(&combined);
                        t.scheme_name = name;
                        if !folio.is_empty() {
                            t.folio = folio;
                        }
                        if !isin.is_empty() {
                            t.isin = isin;
                        }
                    }
                } else {
                    skipped += 1;
                }
            }
        }
    }

    // Commit the last pending transaction
    if let Some(t) = pending {
        transactions.push(t);
    }

    Ok(ChoiceMfParseResult {
        total_rows: transactions.len(),
        skipped_rows: skipped,
        transactions,
    })
}

// ─── Import ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ChoiceMfImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub auto_created_instruments: usize,
}

/// Bulk-insert parsed Choice MF transactions into the DB for a given account.
/// Matches by ISIN; auto-creates instrument records for unknown ISINs.
/// Deduplicates by (account_id, instrument_id, trade_date, nav_paise, txn_type).
#[tauri::command]
pub fn import_choice_mf_transactions(
    account_id: i64,
    transactions: Vec<ParsedMfTransaction>,
) -> Result<ChoiceMfImportResult, String> {
    let conn = db::acquire()?;
    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut auto_created = 0usize;

    let mf_type_id: i64 = conn
        .query_row(
            "SELECT instrument_type_id FROM instrument_types WHERE name='EQUITY_MF' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(2);

    for txn in &transactions {
        // SWITCH_OUT is the mirror of SWITCH_IN — skip to avoid double-counting
        if txn.txn_type == "SWITCH_OUT" {
            skipped += 1;
            continue;
        }

        // Resolve instrument by ISIN
        let mut instrument_id: Option<i64> = if !txn.isin.is_empty() {
            conn.query_row(
                "SELECT instrument_id FROM instruments WHERE isin = ?1 LIMIT 1",
                [&txn.isin],
                |row| row.get(0),
            )
            .ok()
        } else {
            None
        };

        // Auto-create if not found
        if instrument_id.is_none() && !txn.scheme_name.is_empty() {
            let isin_val: Option<&str> = if txn.isin.is_empty() {
                None
            } else {
                Some(&txn.isin)
            };
            conn.execute(
                "INSERT OR IGNORE INTO instruments (isin, name, instrument_type_id, source)
                 VALUES (?1, ?2, ?3, 'IMPORT')",
                rusqlite::params![isin_val, txn.scheme_name, mf_type_id],
            )
            .map_err(|e| e.to_string())?;

            instrument_id = if !txn.isin.is_empty() {
                conn.query_row(
                    "SELECT instrument_id FROM instruments WHERE isin = ?1 LIMIT 1",
                    [&txn.isin],
                    |row| row.get(0),
                )
                .ok()
            } else {
                conn.query_row(
                    "SELECT instrument_id FROM instruments WHERE name = ?1 ORDER BY instrument_id DESC LIMIT 1",
                    [&txn.scheme_name],
                    |row| row.get(0),
                )
                .ok()
            };

            if instrument_id.is_some() {
                auto_created += 1;
            }
        }

        let instrument_id = match instrument_id {
            Some(id) => id,
            None => {
                skipped += 1;
                continue;
            }
        };

        let nav_paise = (txn.nav_rs * 100.0).round() as i64;

        // Deduplicate
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions
                 WHERE account_id=?1 AND instrument_id=?2 AND trade_date=?3
                   AND price_paise=?4 AND txn_type=?5",
                rusqlite::params![
                    account_id,
                    instrument_id,
                    txn.trade_date,
                    nav_paise,
                    txn.txn_type
                ],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if exists {
            skipped += 1;
            continue;
        }

        let gross = (txn.units * txn.nav_rs * 100.0).round() as i64;
        let total_value_paise = match txn.txn_type.as_str() {
            "BUY" | "SIP" | "SWITCH_IN" => -gross,
            _ => gross,
        };
        let notes: Option<String> = if txn.folio.is_empty() {
            None
        } else {
            Some(format!("Folio: {}", txn.folio))
        };

        conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, txn_type, trade_segment, trade_date,
                 quantity, price_paise, brokerage_paise, stt_paise, other_charges_paise,
                 total_value_paise, notes)
             VALUES (?1,?2,?3,'MF',?4,?5,?6,0,0,0,?7,?8)",
            rusqlite::params![
                account_id,
                instrument_id,
                txn.txn_type,
                txn.trade_date,
                txn.units,
                nav_paise,
                total_value_paise,
                notes,
            ],
        )
        .map_err(|e| e.to_string())?;

        imported += 1;
    }

    Ok(ChoiceMfImportResult {
        imported,
        skipped,
        auto_created_instruments: auto_created,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_mf_pdf() {
        let path = "/home/dmachine/workspace/Transactions_Report_Individual_10_04_2026__16_27_08.pdf";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let result = parse_choice_mf_pdf(path.to_string()).expect("parse failed");
        println!("MF Total: {}, Skipped: {}", result.total_rows, result.skipped_rows);
        for t in result.transactions.iter().take(5) {
            println!("  {} | {} | {} | amount={} nav={} units={} isin={}",
                t.trade_date, t.txn_type, t.scheme_name,
                t.amount_rs, t.nav_rs, t.units, t.isin);
        }
        assert!(result.total_rows > 0, "expected transactions, got 0");
        // All transactions should have a date
        for t in &result.transactions {
            assert!(!t.trade_date.is_empty(), "empty trade date");
            assert!(t.amount_rs > 0.0, "zero amount for {}", t.scheme_name);
        }
    }
}
