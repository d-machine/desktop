//! Parser for Choice Wealth PVT LTD MF Transaction Report PDF.
//!
//! Supports two report formats emitted by the Choice Wealth portal:
//!
//!  Format A — 7-column (older "Individual" reports):
//!    Date | Transaction Type | Security/Scheme/Folio/ISIN | Amount | NAV | Units | Current Value
//!    Column x-origins (points): 26, 168, 310, 619, 717, 804, 912
//!
//!  Format B — 9-column (newer "D03695" / broker reports):
//!    Date | Family Head | Client Name | Transaction Type | Security/Scheme/Folio/ISIN | Amount | NAV | Units | Current Value
//!    Column x-origins (points): 26, 132, 239, 346, 452, 699, 770, 831, 912
//!
//! The format is auto-detected from the header row of page 1.
//!
//! Both formats produce multi-line rows where:
//!  - The Transaction Type may span 2-4 continuation lines
//!  - The Folio/ISIN always appears on a continuation line of the Scheme column

use crate::{commands::import::pdf_utils, db};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ─── Extraction parameters ────────────────────────────────────────────────────

const X_GAP:      f32 = 6.0;
const CHAR_Y_TOL: f32 = 2.0;
const ROW_Y_TOL:  f32 = 5.0;

// ─── Column definitions ───────────────────────────────────────────────────────

/// Format A: 7-column (older individual reports — no Family/Client columns)
/// Col indices: 0=Date, 1=TxnType, 2=Scheme/ISIN, 3=Amount, 4=NAV, 5=Units, 6=CurVal
const COL_X_A: [f32; 7] = [20.0, 162.0, 304.0, 613.0, 710.0, 797.0, 905.0];

/// Format B: 9-column (D03695 broker reports with Family Head + Client Name columns)
/// Col indices: 0=Date, 1=FamilyHead(skip), 2=ClientName(skip), 3=TxnType, 4=Scheme/ISIN, 5=Amount, 6=NAV, 7=Units, 8=CurVal
const COL_X_B: [f32; 9] = [20.0, 126.0, 233.0, 340.0, 447.0, 693.0, 762.0, 825.0, 905.0];

#[derive(Clone, Copy, Debug)]
enum PdfFormat { A, B }

impl PdfFormat {
    fn col_x(self) -> &'static [f32] {
        match self {
            PdfFormat::A => &COL_X_A,
            PdfFormat::B => &COL_X_B,
        }
    }
    /// Column indices into the cells array
    fn idx(self) -> (usize, usize, usize, usize, usize) {
        // (type, scheme, amount, nav, units)
        match self {
            PdfFormat::A => (1, 2, 3, 4, 5),
            PdfFormat::B => (3, 4, 5, 6, 7),
        }
    }
}

// ─── Regex helpers ────────────────────────────────────────────────────────────

fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches a date anywhere in the string (cell may have trailing name text in Format B)
    RE.get_or_init(|| Regex::new(r"\b(\d{1,2}/\d{1,2}/\d{4})\b").unwrap())
}

fn isin_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(IN[A-Z0-9]{10})\b").unwrap())
}

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedMfTransaction {
    pub trade_date:  String,
    pub scheme_name: String,
    pub isin:        String,
    pub folio:       String,
    /// Normalised: "BUY", "SIP", "REDEMPTION", "SWITCH_IN", "SWITCH_OUT", "DIVIDEND"
    pub txn_type:    String,
    pub amount_rs:   f64,
    pub nav_rs:      f64,
    pub units:       f64,
    /// Original type string from the report
    pub raw_type:    String,
}

#[derive(Debug, Serialize)]
pub struct ChoiceMfParseResult {
    pub transactions: Vec<ParsedMfTransaction>,
    pub total_rows:   usize,
    pub skipped_rows: usize,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn classify_txn_type(raw: &str) -> &'static str {
    let t = raw.to_uppercase();
    if t.contains("FRESH") || t.contains("NEW") {
        "BUY"
    } else if t.contains("REDEMPTION") || t.contains("REDEEM") {
        "REDEMPTION"
    } else if t.contains("SWITCH IN") || t.contains("SWITCH-IN") || t.contains("SWITCH_IN") {
        "SWITCH_IN"
    } else if t.contains("SWITCH OUT") || t.contains("SWITCH-OUT") || t.contains("SWITCH_OUT") {
        "SWITCH_OUT"
    } else if t.contains("DIVIDEND") || t.contains("IDCW") {
        "DIVIDEND"
    } else if t.contains("ADDITIONAL") || t.contains("SYSTEMATIC") || t.contains("SIP") {
        "SIP"
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

/// Extract (scheme_name, folio, isin) from a field that may be multi-line.
/// The field may look like: "SBI ELSS Tax Saver Reg-G\n40005103/INF200K01495"
/// or for HDFC Mid Cap: "HDFC Mid Cap Reg-G\n30831992/23/INF179K01CR2"
fn extract_isin_folio(scheme_field: &str) -> (String, String, String) {
    let lines: Vec<&str> = scheme_field
        .lines()
        .map(str::trim)
        .filter(|l| !l.is_empty())
        .collect();

    if lines.is_empty() {
        return (String::new(), String::new(), String::new());
    }

    // Find the folio/ISIN line — the one that contains an ISIN (12-char IN* code)
    let re = isin_re();
    let mut scheme_lines: Vec<&str> = Vec::new();
    let mut isin = String::new();
    let mut folio = String::new();

    for line in &lines {
        if let Some(cap) = re.captures(line) {
            isin = cap[1].to_string();
            // Everything before the ISIN in this line is part of the folio
            let before_isin = &line[..cap.get(1).unwrap().start()];
            folio = before_isin.trim_end_matches('/').trim().to_string();
            // Remove trailing slash-segments that are part of the folio path
        } else {
            scheme_lines.push(line);
        }
    }

    let scheme_name = scheme_lines.join(" ").trim().to_string();
    (scheme_name, folio, isin)
}

/// Detect PDF format by scanning the header row of page 1.
/// Returns Format B if we see a "Family Head" span at x < 200, else Format A.
/// The Rust PDF extractor merges adjacent same-line text into single spans, so
/// "Family Head Name" appears as one span (not three separate words).
fn detect_format(page_spans: &[pdf_utils::TextSpan]) -> PdfFormat {
    // Format B: the TABLE COLUMN HEADER "Family Head Name" appears at x≈132.
    // Format A also has "Family Head Name: ..." in the client info section but at x≈165.
    // Using x < 150 cleanly separates the two cases.
    for span in page_spans {
        let t = span.text.to_lowercase();
        if t.contains("family head") && span.x < 150.0 {
            return PdfFormat::B;
        }
    }
    PdfFormat::A
}

// ─── Main parser ─────────────────────────────────────────────────────────────

/// Parse a Choice Wealth MF Transaction Report PDF.
#[tauri::command]
pub fn parse_choice_mf_pdf(file_path: String, password: Option<String>) -> Result<ChoiceMfParseResult, String> {
    let all_pages = pdf_utils::extract_all_page_spans_pwd_cfg(&file_path, password.as_deref(), X_GAP, CHAR_Y_TOL)?;

    // Detect format from page 1
    let fmt = all_pages.first()
        .map(|spans| detect_format(spans))
        .unwrap_or(PdfFormat::A);

    let col_x  = fmt.col_x();
    let (i_type, i_scheme, i_amount, i_nav, i_units) = fmt.idx();
    let ncols  = col_x.len();

    let mut transactions: Vec<ParsedMfTransaction> = Vec::new();
    let mut skipped = 0usize;
    let mut pending: Option<ParsedMfTransaction>   = None;

    for page_spans in all_pages {
        let rows = pdf_utils::page_spans_to_rows(page_spans, ROW_Y_TOL);

        for row in rows {
            if row.is_empty() {
                continue;
            }

            let mut cells = pdf_utils::spans_to_cells(&row, col_x);
            cells.resize(ncols, String::new());

            let raw_date   = cells[0].trim().to_string();
            let type_str   = cells[i_type].trim().to_string();
            let scheme_str = cells[i_scheme].trim().to_string();
            let amount_str = cells[i_amount].trim().to_string();
            let nav_str    = cells[i_nav].trim().to_string();
            let units_str  = cells[i_units].trim().to_string();

            // Extract just the date token from the cell (Format B may have name text appended)
            let date_match = date_re().captures(&raw_date);
            let date_str   = date_match.as_ref().map(|c| c[1].to_string()).unwrap_or_default();

            // Skip header / summary / page-number rows
            if raw_date.to_lowercase().contains("transaction date")
                || raw_date.to_lowercase().contains("page no")
            {
                continue;
            }
            // Skip the summary table at the end ("Additional", "Fresh/Buy", amounts)
            if raw_date.to_lowercase().contains("additional")
                || raw_date.to_lowercase().contains("fresh")
            {
                continue;
            }

            let is_date = !date_str.is_empty();
            let amount  = parse_amount(&amount_str);

            if is_date && amount > 0.0 {
                // Commit the previous pending transaction
                if let Some(t) = pending.take() {
                    transactions.push(t);
                }
                // Start a new transaction
                let (scheme_name, folio, isin) = extract_isin_folio(&scheme_str);
                pending = Some(ParsedMfTransaction {
                    trade_date:  parse_date(&date_str),
                    scheme_name,
                    isin,
                    folio,
                    txn_type:    classify_txn_type(&type_str).to_string(),
                    amount_rs:   amount,
                    nav_rs:      parse_amount(&nav_str),
                    units:       parse_amount(&units_str),
                    raw_type:    type_str,
                });
            } else if is_date {
                // Date present but amount = 0 — shouldn't normally happen; skip
                skipped += 1;
            } else if !raw_date.is_empty() {
                // raw_date has something but no date pattern — probably a header/summary row
                continue;
            } else {
                // No date → continuation line
                if let Some(ref mut t) = pending {
                    // Append Transaction Type continuation text
                    if !type_str.is_empty() {
                        if !t.raw_type.is_empty() {
                            t.raw_type.push(' ');
                        }
                        t.raw_type.push_str(&type_str);
                        t.txn_type = classify_txn_type(&t.raw_type).to_string();
                    }
                    // Append Scheme / Folio / ISIN continuation
                    if !scheme_str.is_empty() {
                        if t.isin.is_empty() {
                            // Try to extract ISIN from the continuation line
                            let combined = format!("{}\n{}", t.scheme_name, scheme_str);
                            let (name, folio, isin) = extract_isin_folio(&combined);
                            if !name.is_empty() { t.scheme_name = name; }
                            if !folio.is_empty() { t.folio = folio; }
                            if !isin.is_empty()  { t.isin  = isin;  }
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
        total_rows:   transactions.len(),
        skipped_rows: skipped,
        transactions,
    })
}

// ─── Import ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ChoiceMfImportResult {
    pub imported:                  usize,
    pub skipped:                   usize,
    pub auto_created_instruments:  usize,
}

/// Bulk-insert parsed Choice MF transactions into the DB for a given account.
/// Matches by ISIN; auto-creates instrument records for unknown ISINs.
/// Deduplicates by (account_id, instrument_id, trade_date, nav_paise, txn_type).
#[tauri::command]
pub fn import_choice_mf_transactions(
    account_id:   i64,
    transactions: Vec<ParsedMfTransaction>,
) -> Result<ChoiceMfImportResult, String> {
    let conn = db::acquire()?;
    let mut imported     = 0usize;
    let mut skipped      = 0usize;
    let mut auto_created = 0usize;

    let _mf_type_id: i64 = conn
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
        // Skip rows with no meaningful data
        if txn.amount_rs <= 0.0 && txn.units <= 0.0 {
            skipped += 1;
            continue;
        }

        // Resolve instrument locally by ISIN
        let resolved_id: Option<i64> = if !txn.isin.is_empty() {
            conn.query_row(
                "SELECT instrument_id FROM instrument_equity WHERE isin = ?1 LIMIT 1",
                [&txn.isin],
                |row| row.get(0),
            )
            .ok()
        } else {
            None
        };

        // If not found locally, stage in pending_instruments for async server resolution
        let (instrument_id, pending_instrument_id): (Option<i64>, Option<i64>) =
            if let Some(id) = resolved_id {
                (Some(id), None)
            } else if !txn.scheme_name.is_empty() {
                let isin_val: Option<&str> = if txn.isin.is_empty() { None } else { Some(&txn.isin) };
                conn.execute(
                    "INSERT INTO pending_instruments (instrument_type, name, isin)
                     VALUES ('EQUITY_MF', ?1, ?2)",
                    rusqlite::params![txn.scheme_name, isin_val],
                )
                .map_err(|e| e.to_string())?;
                auto_created += 1;
                (None, Some(conn.last_insert_rowid()))
            } else {
                skipped += 1;
                continue;
            };

        let nav_paise = (txn.nav_rs * 100.0).round() as i64;

        // Deduplicate: same account + instrument + date + NAV + type
        // Dedup only meaningful for resolved instruments (pending have no prior txns)
        if let Some(iid) = instrument_id {
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM transactions
                     WHERE account_id=?1 AND instrument_id=?2 AND trade_date=?3
                       AND price_paise=?4 AND txn_type=?5",
                    rusqlite::params![account_id, iid, txn.trade_date, nav_paise, txn.txn_type],
                    |row| row.get::<_, i64>(0),
                )
                .map(|c| c > 0)
                .unwrap_or(false);
            if exists { skipped += 1; continue; }
        }

        // Cash flow sign: BUY/SIP/SWITCH_IN = outflow (negative), REDEMPTION = inflow
        let gross             = (txn.units * txn.nav_rs * 100.0).round() as i64;
        let total_value_paise = match txn.txn_type.as_str() {
            "BUY" | "SIP" | "SWITCH_IN" => -gross,
            _                            =>  gross,
        };
        let notes: Option<String> = if txn.folio.is_empty() {
            None
        } else {
            Some(format!("Folio: {}", txn.folio))
        };

        conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, pending_instrument_id, txn_type, trade_segment,
                 trade_date, quantity, price_paise, brokerage_paise, stt_paise,
                 other_charges_paise, total_value_paise, notes)
             VALUES (?1,?2,?3,?4,'MF',?5,?6,?7,0,0,0,?8,?9)",
            rusqlite::params![
                account_id, instrument_id, pending_instrument_id, txn.txn_type,
                txn.trade_date, txn.units, nav_paise,
                total_value_paise, notes,
            ],
        )
        .map_err(|e| e.to_string())?;

        imported += 1;
    }

    Ok(ChoiceMfImportResult { imported, skipped, auto_created_instruments: auto_created })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn run_pdf(path: &str) {
        if !std::path::Path::new(path).exists() {
            eprintln!("SKIP: file not found: {path}");
            return;
        }
        let result = parse_choice_mf_pdf(path.to_string(), None).expect("parse failed");
        println!("File: {path}");
        println!("  Total: {}  Skipped: {}", result.total_rows, result.skipped_rows);
        for t in result.transactions.iter().take(5) {
            println!("  {} | {:10} | {:<45} | amount={:8.2} nav={:8.4} units={:8.4} isin={}",
                t.trade_date, t.txn_type,
                t.scheme_name.chars().take(45).collect::<String>(),
                t.amount_rs, t.nav_rs, t.units, t.isin);
        }
        assert!(result.total_rows > 0, "expected transactions, got 0");
        for t in &result.transactions {
            assert!(!t.trade_date.is_empty(), "empty trade_date");
            assert!(t.amount_rs > 0.0 || t.units > 0.0, "zero amount and units for {}", t.scheme_name);
        }
    }

    #[test]
    fn test_format_a_old_pdf() {
        run_pdf("/home/dmachine/workspace/Transactions_Report_Individual_10_04_2026__16_27_08.pdf");
    }

    #[test]
    fn test_format_b_d03695_pdf1() {
        run_pdf("/home/dmachine/workspace/D03695_Transaction (1).pdf");
    }

    #[test]
    fn test_format_b_d03695_pdf2() {
        run_pdf("/home/dmachine/workspace/D03695_Transaction (2).pdf");
    }
}
