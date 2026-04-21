//! Parser for Choice Equity Broking PVT LTD — Global Details Report PDF.
//!
//! PDF layout (A4 landscape, ≈ 842 × 595 pt):
//!
//!   Section headers:  full-width row "Security Name - BSE Code"
//!   Data rows:        9 columns:
//!     Col 0 (x≈  0): Security name (repeated, ignored — we use section header)
//!     Col 1 (x≈130): Date  (DD-Mon-YYYY, sometimes wraps as "DD-Mon-YYY\nY")
//!     Col 2 (x≈196): Buy Qty
//!     Col 3 (x≈248): Buy Price
//!     Col 4 (x≈306): Sell Qty
//!     Col 5 (x≈356): Sell Price
//!     Col 6 (x≈415): Net Qty   (positive=net buy, negative=net sell, 0=intraday)
//!     Col 7 (x≈462): Net Price
//!     Col 8 (x≈512): Net Value (P&L)
//!   TOTAL rows:       col 1 = "TOTAL", aggregates
//!
//! A "logical row" in this PDF spans 3 physical lines (security name first
//! part, financial data, security name BSE code continuation) with Y gaps of
//! ~4.6 pt — captured by the rolling-window row grouper with tolerance 5 pt.

use crate::{commands::import::{pdf_utils, flag_oversells}, db};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

const EQ_COL_X: [f32; 9] = [0.0, 130.0, 196.0, 248.0, 306.0, 356.0, 415.0, 462.0, 512.0];

fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\d{2}-[A-Za-z]{3}-\d{4}").unwrap())
}

fn section_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(.+?)\s*-\s*(\d+)\s*$").unwrap())
}

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedEquityTrade {
    pub trade_date: String,
    pub security_name: String,
    pub exchange_code: String,
    /// "BUY" or "SELL"
    pub txn_type: String,
    pub quantity: f64,
    pub price: f64,
    /// "EQ" for delivery trades, "INTRADAY" for same-day round-trips
    pub trade_segment: String,
}

/// A row that could not be automatically parsed and needs user review.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SkippedRow {
    pub security_name: String,
    pub raw_date: String,
    pub buy_qty: String,
    pub buy_price: String,
    pub sell_qty: String,
    pub sell_price: String,
    pub reason: String,
}

#[derive(Debug, Serialize)]
pub struct ChoiceEquityParseResult {
    pub transactions: Vec<ParsedEquityTrade>,
    pub total_rows: usize,
    pub skipped_rows: usize,
    pub intraday_rows: usize,
    /// Client ID extracted from the PDF header (e.g. "D03695"), if present.
    pub client_id: Option<String>,
    /// Client name extracted from the PDF header (e.g. "SUMIT KHAITAN"), if present.
    pub client_name: Option<String>,
    /// Rows that were skipped because they could not be parsed automatically.
    /// Shown to the user so they can manually review/correct.
    pub skipped_details: Vec<SkippedRow>,
}

// ─── Small value parsers ──────────────────────────────────────────────────────

fn month_num(abbr: &str) -> Option<u32> {
    match abbr.to_lowercase().as_str() {
        "jan" => Some(1),  "feb" => Some(2),  "mar" => Some(3),
        "apr" => Some(4),  "may" => Some(5),  "jun" => Some(6),
        "jul" => Some(7),  "aug" => Some(8),  "sep" => Some(9),
        "oct" => Some(10), "nov" => Some(11), "dec" => Some(12),
        _ => None,
    }
}

/// Parse "18-Jul-2025" → "2025-07-18".
/// Handles wrapped dates like "18-Jul-202 5" by removing all whitespace first.
fn parse_equity_date(raw: &str) -> Option<String> {
    let clean: String = raw.split_whitespace().collect();
    let caps = date_re().captures(&clean)?;
    let full = &caps[0];
    let parts: Vec<&str> = full.splitn(3, '-').collect();
    if parts.len() != 3 { return None; }
    let day: u32   = parts[0].parse().ok()?;
    let month      = month_num(parts[1])?;
    let year: u32  = parts[2].parse().ok()?;
    Some(format!("{}-{:02}-{:02}", year, month, day))
}

fn parse_qty(s: &str) -> f64 {
    let s = s.trim();
    if s == "-" || s.is_empty() { return 0.0; }
    s.replace(',', "").trim().parse().unwrap_or(0.0)
}

fn parse_price(s: &str) -> f64 {
    let s = s.trim();
    if s == "-" || s.is_empty() { return 0.0; }
    s.replace(',', "").trim().parse().unwrap_or(0.0)
}

// ─── PDF metadata extraction ──────────────────────────────────────────────────

/// Scan rows for a labelled field, e.g. ["Client ID", ":  D03695", …].
fn extract_header_field(rows: &[Vec<pdf_utils::TextSpan>], key: &str) -> Option<String> {
    let key_lc = key.to_lowercase();
    for row in rows {
        for (i, span) in row.iter().enumerate() {
            if span.text.trim().to_lowercase() == key_lc {
                if let Some(next) = row.get(i + 1) {
                    let val = next.text.trim_start_matches(':').trim().to_string();
                    if !val.is_empty() { return Some(val); }
                }
            }
        }
    }
    None
}

/// Extract (client_id, client_name) from the first-page header spans.
fn extract_client_metadata(
    all_pages: &[Vec<pdf_utils::TextSpan>],
) -> (Option<String>, Option<String>) {
    let header_rows: Vec<Vec<pdf_utils::TextSpan>> = all_pages
        .first()
        .map(|page| pdf_utils::page_spans_to_rows(page.clone(), 5.0))
        .unwrap_or_default();
    let client_id   = extract_header_field(&header_rows, "Client ID");
    let client_name = extract_header_field(&header_rows, "Name");
    (client_id, client_name)
}

// ─── Step 1: Flatten pages → cell rows ───────────────────────────────────────

/// Fix the "column bleed" edge case: when a date's x-position in the PDF falls
/// just below the col0/col1 boundary, `spans_to_cells` assigns it to col0,
/// producing e.g. "TATA STEEL LTD. - 50047009-Jul-2024" with col1 empty.
/// This function splits the date back out into col1.
fn fix_col0_date_bleed(cells: &mut Vec<String>) {
    if !cells[1].trim().is_empty() {
        return; // col1 already has content — nothing to fix
    }
    let re = date_re();
    let Some(m) = re.find(&cells[0]) else { return };
    // Only split when the char immediately before the date is a digit
    // (the last digit of the BSE code running straight into the date).
    let before = &cells[0][..m.start()];
    if !before.ends_with(|c: char| c.is_ascii_digit()) {
        return;
    }
    let name_part = before.trim().to_string();
    let date_part = m.as_str().to_string();
    cells[0] = name_part;
    cells[1] = date_part;
}

/// Flatten all PDF pages into a single list of 9-cell rows, applying
/// `fix_col0_date_bleed` to each row as it is produced.
fn flatten_pages_to_cells(all_pages: &[Vec<pdf_utils::TextSpan>]) -> Vec<Vec<String>> {
    all_pages
        .iter()
        .flat_map(|page| {
            pdf_utils::page_spans_to_rows(page.clone(), 5.0)
                .into_iter()
                .filter(|r| !r.is_empty())
                .map(|r| {
                    let mut cells = pdf_utils::spans_to_cells(&r, &EQ_COL_X);
                    cells.resize(9, String::new());
                    fix_col0_date_bleed(&mut cells);
                    cells
                })
        })
        .collect()
}

// ─── Step 2: Merge rows split across page breaks ──────────────────────────────

/// Returns true if `cells` is a page or column header row that belongs to the
/// PDF layout and should be skipped during the completion scan.
///
/// These rows include: TOTAL aggregates, "Security / Date" column labels,
/// "Buy / Sell / Net / Qty / Price" labels, client-ID page headers, and the
/// "Global Details Report N/M" page-info row.
fn is_layout_row(cells: &[String]) -> bool {
    let col0 = cells[0].trim();
    let col1 = cells[1].trim();
    col1.to_uppercase() == "TOTAL"
        || col1.to_lowercase().contains("date")
        || col0 == "Security"
        || col0.starts_with("D0")                                      // "D03695 SUMIT KHAITAN"
        || (!col0.is_empty() && cells[8].trim().to_lowercase().contains("print")) // page header
        || cells[4].trim().to_lowercase().starts_with("global")        // "Global Details Report N/M"
        || cells[2].trim() == "Buy"
        || cells[2].trim() == "Qty"
}

/// Merge rows that were split across a PDF page break.
///
/// Two cases are handled:
///
/// 1. **Incomplete section header** — col0 has a partial security name (no BSE
///    code suffix), all other cols empty.  The completion row supplies the rest
///    of col0 only.
///
/// 2. **Incomplete data row** — has trading data in cols 2+, but col0 may be
///    missing the BSE code suffix and/or col1 has a truncated date.  The
///    completion row supplies the missing col0 tail and/or the missing col1 tail.
///
/// Layout rows (page headers, column labels, TOTAL) are skipped during the
/// forward scan.  Scanning stops when a genuine data row or a complete section
/// header is reached.
fn merge_split_rows(all_cells: &mut Vec<Vec<String>>) {
    let mut i = 0;
    while i < all_cells.len() {
        let col0 = all_cells[i][0].trim().to_string();
        let col1 = all_cells[i][1].trim().to_string();
        let has_trading      = all_cells[i][2..].iter().any(|c| !c.trim().is_empty());
        let all_other_empty  = all_cells[i][1..].iter().all(|c| c.trim().is_empty());

        let is_section_hdr   = !col0.is_empty() && all_other_empty;
        let is_data_row      = !col1.is_empty() && has_trading;

        let incomplete_section_hdr = is_section_hdr && !section_re().is_match(&col0);
        let incomplete_data_sec    = is_data_row && !col0.is_empty() && !section_re().is_match(&col0);
        let incomplete_data_date   = is_data_row && parse_equity_date(&col1).is_none();

        if incomplete_section_hdr || incomplete_data_sec || incomplete_data_date {
            let found_j = find_completion_row(
                all_cells, i,
                incomplete_section_hdr,
                incomplete_data_sec,
                incomplete_data_date,
                &col0, &col1,
            );

            if let Some(j) = found_j {
                let next = all_cells.remove(j);
                let next_is_data = next[2..].iter().any(|c| !c.trim().is_empty())
                    && !next[1].trim().is_empty();

                if incomplete_section_hdr && next_is_data {
                    // 3-line grouping failure: the PDF split one logical data row
                    // into three physical lines with Y-gaps too large to group.
                    //   row i   = first name fragment  (e.g. "BHARTIYA")
                    //   next    = rest of name + date + data  (e.g. "INTERNATIONAL LTD. -", date, ...)
                    //   (BSE code row follows and will be merged on the next pass)
                    // Prepend col0 into next, replace row i with next, re-examine.
                    let mut merged = next;
                    merged[0] = format!("{} {}", col0.trim(), merged[0].trim());
                    all_cells[i] = merged;
                } else {
                    // Normal page-break merge: append missing tail into row i.
                    if (incomplete_section_hdr || incomplete_data_sec) && !next[0].trim().is_empty() {
                        all_cells[i][0] = format!("{} {}", all_cells[i][0].trim(), next[0].trim());
                    }
                    if incomplete_data_date && !next[1].trim().is_empty() {
                        all_cells[i][1] = format!("{}{}", all_cells[i][1].trim(), next[1].trim());
                    }
                }
                // Re-examine the merged row — it may still need another pass.
                continue;
            }
        }
        i += 1;
    }
}

/// Scan forward from row `i` for the first row that can complete the missing
/// fields.  Returns the index of the completion row, or `None`.
fn find_completion_row(
    all_cells: &[Vec<String>],
    i: usize,
    incomplete_section_hdr: bool,
    incomplete_data_sec: bool,
    incomplete_data_date: bool,
    col0: &str,
    col1: &str,
) -> Option<usize> {
    for j in (i + 1)..all_cells.len().min(i + 20) {
        let next_col0            = all_cells[j][0].trim().to_string();
        let next_col1            = all_cells[j][1].trim().to_string();
        let next_has_trading     = all_cells[j][2..].iter().any(|c| !c.trim().is_empty());
        let next_all_other_empty = all_cells[j][1..].iter().all(|c| c.trim().is_empty());

        // Stop at a genuine data row or a complete section header.
        if next_has_trading && !next_col1.is_empty() {
            // Exception: 3-line grouping failure. An incomplete section header
            // may be immediately followed by a data row whose col0 is also a
            // partial security name (no BSE code). In that case, merge col0
            // fragments rather than stopping.
            if incomplete_section_hdr
                && !next_col0.is_empty()
                && !section_re().is_match(&next_col0)
            {
                return Some(j);
            }
            break;
        }
        if !next_col0.is_empty() && next_all_other_empty && section_re().is_match(&next_col0) {
            break;
        }

        // Skip layout rows — they are not candidates and not terminators.
        if is_layout_row(&all_cells[j]) {
            continue;
        }

        // Must have content in col0 or col1 and no trading data.
        if next_has_trading || (next_col0.is_empty() && next_col1.is_empty()) {
            continue;
        }

        // Section header: completion only via col0.
        if incomplete_section_hdr {
            if !next_col0.is_empty() {
                let combined = format!("{} {}", col0, next_col0);
                if section_re().is_match(combined.trim()) {
                    return Some(j);
                }
            }
            continue;
        }

        // Data row: both col0 (security name) and col1 (date) must be satisfied.
        let sec_ok = if incomplete_data_sec && !next_col0.is_empty() {
            let combined = format!("{} {}", col0, next_col0);
            section_re().is_match(combined.trim())
        } else {
            !incomplete_data_sec
        };

        let date_ok = if incomplete_data_date && !next_col1.is_empty() {
            let combined = format!("{}{}", col1, next_col1);
            parse_equity_date(&combined).is_some()
        } else {
            !incomplete_data_date
        };

        if sec_ok && date_ok {
            return Some(j);
        }
    }
    None
}

// ─── Step 3: Parse cell rows → trades ────────────────────────────────────────

struct ParseOutput {
    trades:          Vec<ParsedEquityTrade>,
    skipped:         usize,
    intraday:        usize,
    skipped_details: Vec<SkippedRow>,
}

/// Iterate the merged cell list and emit `ParsedEquityTrade` records.
fn parse_rows(all_cells: Vec<Vec<String>>) -> ParseOutput {
    let mut trades:          Vec<ParsedEquityTrade> = Vec::new();
    let mut skipped          = 0usize;
    let mut intraday         = 0usize;
    let mut skipped_details: Vec<SkippedRow>        = Vec::new();
    let mut cur_security     = String::new();
    let mut cur_bse_code     = String::new();

    for cells in all_cells {
        let col0 = cells[0].trim().to_string();
        let col1 = cells[1].trim().to_string();
        let col2 = cells[2].trim().to_string();
        let col3 = cells[3].trim().to_string();
        let col4 = cells[4].trim().to_string();
        let col5 = cells[5].trim().to_string();

        // Skip report / column header rows.
        if col1.to_lowercase().contains("date") { continue; }
        if col0.to_lowercase().contains("dear")
            || col0.to_lowercase().contains("please")
            || col0.to_lowercase().contains("statement")
        { continue; }

        // TOTAL row.
        if col1.to_uppercase() == "TOTAL" { continue; }

        // Section header: only col0 has content.
        let other_empty = cells[1..].iter().all(|c| c.trim().is_empty());
        if other_empty && !col0.is_empty() {
            if let Some(caps) = section_re().captures(&col0) {
                cur_security = caps[1].trim().to_string();
                cur_bse_code = caps[2].trim().to_string();
            } else {
                // Partial header the merge pass couldn't fix — reset to avoid
                // misattributing subsequent rows to the previous security.
                cur_security.clear();
                cur_bse_code.clear();
            }
            continue;
        }

        // Data row — must be inside a known section.
        if cur_security.is_empty() { skipped += 1; continue; }

        let date = match parse_equity_date(&col1) {
            Some(d) => d,
            None => {
                if !col1.is_empty() {
                    skipped_details.push(SkippedRow {
                        security_name: cur_security.clone(),
                        raw_date:  col1.clone(),
                        buy_qty:   col2.clone(),
                        buy_price: col3.clone(),
                        sell_qty:  col4.clone(),
                        sell_price: col5.clone(),
                        reason: format!("Could not parse date \"{}\"", col1),
                    });
                }
                skipped += 1;
                continue;
            }
        };

        let buy_qty    = parse_qty(&col2);
        let buy_price  = parse_price(&col3);
        let sell_qty   = parse_qty(&col4);
        let sell_price = parse_price(&col5);

        if buy_qty > 0.0 && sell_qty == 0.0 {
            trades.push(ParsedEquityTrade {
                trade_date: date, security_name: cur_security.clone(),
                exchange_code: cur_bse_code.clone(), txn_type: "BUY".to_string(),
                quantity: buy_qty, price: buy_price, trade_segment: "EQ".to_string(),
            });
        } else if sell_qty > 0.0 && buy_qty == 0.0 {
            trades.push(ParsedEquityTrade {
                trade_date: date, security_name: cur_security.clone(),
                exchange_code: cur_bse_code.clone(), txn_type: "SELL".to_string(),
                quantity: sell_qty, price: sell_price, trade_segment: "EQ".to_string(),
            });
        } else if buy_qty > 0.0 && sell_qty > 0.0 {
            intraday += 1;
            trades.push(ParsedEquityTrade {
                trade_date: date.clone(), security_name: cur_security.clone(),
                exchange_code: cur_bse_code.clone(), txn_type: "BUY".to_string(),
                quantity: buy_qty, price: buy_price, trade_segment: "INTRADAY".to_string(),
            });
            trades.push(ParsedEquityTrade {
                trade_date: date, security_name: cur_security.clone(),
                exchange_code: cur_bse_code.clone(), txn_type: "SELL".to_string(),
                quantity: sell_qty, price: sell_price, trade_segment: "INTRADAY".to_string(),
            });
        } else {
            skipped_details.push(SkippedRow {
                security_name: cur_security.clone(),
                raw_date:  col1.clone(),
                buy_qty:   col2.clone(), buy_price: col3.clone(),
                sell_qty:  col4.clone(), sell_price: col5.clone(),
                reason: "Both buy and sell quantities are zero".to_string(),
            });
            skipped += 1;
        }
    }

    ParseOutput { trades, skipped, intraday, skipped_details }
}

// ─── Public parse entry point ─────────────────────────────────────────────────

/// Parse a Choice Equity Global Details Report PDF.
#[tauri::command]
pub fn parse_choice_equity_pdf(file_path: String) -> Result<ChoiceEquityParseResult, String> {
    let all_pages = pdf_utils::extract_all_page_spans(&file_path)?;

    let (client_id, client_name) = extract_client_metadata(&all_pages);

    let mut all_cells = flatten_pages_to_cells(&all_pages);
    merge_split_rows(&mut all_cells);
    let out = parse_rows(all_cells);

    Ok(ChoiceEquityParseResult {
        total_rows:      out.trades.len(),
        skipped_rows:    out.skipped,
        intraday_rows:   out.intraday,
        transactions:    out.trades,
        skipped_details: out.skipped_details,
        client_id,
        client_name,
    })
}

// ─── Import ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct SkippedDetail {
    pub trade_date:    String,
    pub security_name: String,
    pub txn_type:      String,
    pub quantity:      f64,
    pub price:         f64,
    pub reason:        String,
}

#[derive(Debug, Serialize)]
pub struct ChoiceEquityImportResult {
    pub imported:                 usize,
    pub skipped:                  usize,
    pub auto_created_instruments: usize,
    pub skipped_details:          Vec<SkippedDetail>,
}

/// Bulk-insert parsed Choice Equity trades into the DB for a given account.
/// Matches by BSE code (stored in instrument_equity.bse_code).
/// Auto-creates placeholder equity instruments for unknown BSE codes.
/// Deduplicates by (account_id, instrument_id, trade_date, price_paise, quantity, txn_type, trade_segment).
#[tauri::command]
pub fn import_choice_equity_trades(
    account_id: i64,
    transactions: Vec<ParsedEquityTrade>,
) -> Result<ChoiceEquityImportResult, String> {
    let conn = db::acquire()?;
    let mut imported        = 0usize;
    let mut skipped         = 0usize;
    let mut auto_created    = 0usize;
    let mut skipped_details: Vec<SkippedDetail> = Vec::new();

    let equity_type_id: i64 = conn
        .query_row(
            "SELECT instrument_type_id FROM instrument_types WHERE name='EQUITY' LIMIT 1",
            [], |row| row.get(0),
        )
        .unwrap_or(1);

    let bse_exchange_id: Option<i64> = conn
        .query_row(
            "SELECT exchange_id FROM exchanges WHERE code='BSE' LIMIT 1",
            [], |row| row.get(0),
        )
        .ok();

    for trade in &transactions {
        // Resolve instrument by BSE code.
        let mut instrument_id: Option<i64> = conn
            .query_row(
                "SELECT i.instrument_id FROM instruments i
                 JOIN instrument_equity ie ON i.instrument_id = ie.instrument_id
                 WHERE ie.bse_code = ?1 LIMIT 1",
                [&trade.exchange_code], |row| row.get(0),
            )
            .ok();

        // Auto-create placeholder if not found.
        if instrument_id.is_none() && !trade.security_name.is_empty() {
            conn.execute(
                "INSERT OR IGNORE INTO instruments (name, instrument_type_id, primary_exchange_id, source)
                 VALUES (?1, ?2, ?3, 'IMPORT')",
                rusqlite::params![trade.security_name, equity_type_id, bse_exchange_id],
            )
            .map_err(|e| e.to_string())?;

            let new_id: Option<i64> = conn
                .query_row(
                    "SELECT instrument_id FROM instruments WHERE name = ?1 ORDER BY instrument_id DESC LIMIT 1",
                    [&trade.security_name], |row| row.get(0),
                )
                .ok();

            if let Some(id) = new_id {
                let _ = conn.execute(
                    "INSERT OR IGNORE INTO instrument_equity (instrument_id, bse_code) VALUES (?1, ?2)",
                    rusqlite::params![id, trade.exchange_code],
                );
                instrument_id = Some(id);
                auto_created += 1;
            }
        }

        let instrument_id = match instrument_id {
            Some(id) => id,
            None => {
                skipped += 1;
                skipped_details.push(SkippedDetail {
                    trade_date:    trade.trade_date.clone(),
                    security_name: trade.security_name.clone(),
                    txn_type:      trade.txn_type.clone(),
                    quantity:      trade.quantity,
                    price:         trade.price,
                    reason:        "Could not create instrument".to_string(),
                });
                continue;
            }
        };

        let price_paise = (trade.price * 100.0).round() as i64;

        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions
                 WHERE account_id=?1 AND instrument_id=?2 AND trade_date=?3
                   AND price_paise=?4 AND quantity=?5 AND txn_type=?6",
                rusqlite::params![
                    account_id, instrument_id, trade.trade_date,
                    price_paise, trade.quantity, trade.txn_type,
                ],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if exists {
            skipped += 1;
            skipped_details.push(SkippedDetail {
                trade_date:    trade.trade_date.clone(),
                security_name: trade.security_name.clone(),
                txn_type:      trade.txn_type.clone(),
                quantity:      trade.quantity,
                price:         trade.price,
                reason:        "Duplicate".to_string(),
            });
            continue;
        }

        let qty              = trade.quantity;
        let gross_paise      = (qty * trade.price * 100.0).round() as i64;
        let total_value_paise = if trade.txn_type == "BUY" { -gross_paise } else { gross_paise };

        conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, txn_type, trade_segment, trade_date,
                 quantity, price_paise, brokerage_paise, stt_paise, other_charges_paise,
                 total_value_paise, notes)
             VALUES (?1,?2,?3,?4,?5,?6,?7,0,0,0,?8,NULL)",
            rusqlite::params![
                account_id, instrument_id, trade.txn_type, trade.trade_segment,
                trade.trade_date, qty, price_paise, total_value_paise,
            ],
        )
        .map_err(|e| e.to_string())?;

        imported += 1;
    }

    drop(conn); // release before flag pass

    // After inserting all trades, run FIFO simulation to flag any oversells.
    flag_oversells(account_id)?;

    Ok(ChoiceEquityImportResult { imported, skipped, auto_created_instruments: auto_created, skipped_details })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const PDF_PATH: &str =
        "/home/dmachine/workspace/D03695_Global_Details_Report_202604131819042 (Copy).pdf";

    fn parse() -> ChoiceEquityParseResult {
        parse_choice_equity_pdf(PDF_PATH.to_string()).expect("parse failed")
    }

    #[test]
    fn test_parse_equity_pdf() {
        if !std::path::Path::new(PDF_PATH).exists() { return; }
        let result = parse();
        println!("Equity Total: {}, Skipped: {}, Intraday: {}",
            result.total_rows, result.skipped_rows, result.intraday_rows);
        assert!(result.total_rows > 0, "expected trades, got 0");
        for t in &result.transactions {
            assert!(!t.trade_date.is_empty(), "empty trade date");
            assert!(t.quantity > 0.0,         "zero quantity for {}", t.security_name);
            assert!(t.price > 0.0,            "zero price for {}",    t.security_name);
        }
    }

    #[test]
    fn test_hcl_transactions() {
        if !std::path::Path::new(PDF_PATH).exists() { return; }
        let result = parse();
        let hcl: Vec<_> = result.transactions.iter()
            .filter(|t| t.security_name.to_lowercase().contains("hcl"))
            .collect();
        println!("HCL transactions ({}):", hcl.len());
        for t in &hcl {
            println!("  {} | {} | {} | {} @ {}", t.trade_date, t.txn_type, t.trade_segment, t.quantity, t.price);
        }
        assert_eq!(hcl.len(), 4, "expected 4 HCL txns (including 19-Nov-2025 SELL)");
    }

    #[test]
    fn test_tata_steel() {
        if !std::path::Path::new(PDF_PATH).exists() { return; }
        let result = parse();
        let tata: Vec<_> = result.transactions.iter()
            .filter(|t| t.security_name.to_lowercase().contains("tata steel"))
            .collect();
        println!("Tata Steel parsed ({})", tata.len());
        assert_eq!(tata.len(), 20, "expected 20 Tata Steel txns");
    }

    #[test]
    fn test_bhartiya() {
        if !std::path::Path::new(PDF_PATH).exists() { return; }
        let result = parse();
        let txns: Vec<_> = result.transactions.iter()
            .filter(|t| t.security_name.to_lowercase().contains("bhartiya"))
            .collect();
        println!("Bhartiya parsed ({}):", txns.len());
        for t in &txns {
            println!("  {} | {} | {} | {} @ {}", t.trade_date, t.txn_type, t.trade_segment, t.quantity, t.price);
        }
        assert_eq!(result.skipped_details.iter()
            .filter(|r| r.security_name.to_lowercase().contains("bhartiya"))
            .count(), 0, "no Bhartiya rows should be skipped");
    }

    #[test]
    fn test_tata_motors() {
        if !std::path::Path::new(PDF_PATH).exists() { return; }
        let result = parse();
        // Dump ALL securities that contain "tata" or "passenger"
        let relevant: Vec<_> = result.transactions.iter()
            .filter(|t| t.security_name.to_lowercase().contains("tata motor")
                     || t.security_name.to_lowercase().contains("passenger"))
            .collect();
        let mut by_sec: std::collections::HashMap<&str, usize> = std::collections::HashMap::new();
        for t in &relevant { *by_sec.entry(&t.security_name).or_default() += 1; }
        for (sec, count) in &by_sec {
            println!("  {} → {} txns", sec, count);
        }

        // Also dump raw cells for "tata motors" and "passenger"
        let all_pages: Vec<Vec<pdf_utils::TextSpan>> = pdf_utils::extract_all_page_spans(PDF_PATH).expect("extract failed");
        let all_cells: Vec<Vec<String>> = all_pages
            .into_iter()
            .flat_map(|page: Vec<pdf_utils::TextSpan>| {
                pdf_utils::page_spans_to_rows(page, 5.0)
                    .into_iter()
                    .filter(|r| !r.is_empty())
                    .map(|r| {
                        let mut cells = pdf_utils::spans_to_cells(&r, &EQ_COL_X);
                        cells.resize(9, String::new());
                        cells
                    })
            })
            .collect();
        for (i, row) in all_cells.iter().enumerate() {
            if row.iter().any(|c: &String| {
                let lc = c.to_lowercase();
                lc.contains("tata motor") || lc.contains("passenger")
            }) {
                let start = i.saturating_sub(1);
                let end = (i + 3).min(all_cells.len());
                for j in start..end {
                    let m = if j == i { ">>>" } else { "   " };
                    println!("{} [{:04}] {:?}", m, j, all_cells[j]);
                }
                println!("---");
            }
        }
    }

    #[test]
    fn test_irfc() {
        if !std::path::Path::new(PDF_PATH).exists() { return; }
        let result = parse();
        let irfc: Vec<_> = result.transactions.iter()
            .filter(|t| t.security_name.to_lowercase().contains("indian railway"))
            .collect();
        println!("IRFC transactions ({}):", irfc.len());
        for t in &irfc {
            println!("  {} | {} | {} | qty={} @ price={}", t.trade_date, t.txn_type, t.trade_segment, t.quantity, t.price);
        }
        // Show manual FIFO avg
        let mut lots: Vec<(f64, f64)> = vec![]; // (price, qty)
        for t in &irfc {
            if t.txn_type == "BUY" && t.trade_segment != "INTRADAY" {
                lots.push((t.price, t.quantity));
            } else if t.txn_type == "SELL" && t.trade_segment != "INTRADAY" {
                let mut qty = t.quantity;
                for lot in lots.iter_mut() {
                    if qty <= 0.0001 { break; }
                    let matched = qty.min(lot.1);
                    lot.1 -= matched;
                    qty -= matched;
                }
                if qty > 0.0001 { println!("  [WARN] unmatched sell qty={}", qty); }
            }
        }
        let rem_qty: f64 = lots.iter().map(|l| l.1).sum();
        let rem_cost: f64 = lots.iter().map(|l| l.0 * l.1).sum();
        println!("FIFO result: qty={} avg={:.2}", rem_qty, if rem_qty > 0.0 { rem_cost / rem_qty } else { 0.0 });
        println!("Remaining lots: {:?}", lots.iter().filter(|l| l.1 > 0.0001).collect::<Vec<_>>());
    }

    #[test]
    fn test_icici_peek() {
        let path = "/home/dmachine/workspace/TRX-Equity_10-04-2025_1498546.PDF";
        if !std::path::Path::new(path).exists() { return; }
        let pages = pdf_utils::extract_all_page_spans(path).expect("extract failed");
        println!("Pages: {}", pages.len());
        for (pi, page) in pages.iter().enumerate().take(2) {
            println!("\n=== PAGE {} ({} spans) ===", pi + 1, page.len());
            let rows = pdf_utils::page_spans_to_rows(page.clone(), 3.0);
            for row in &rows {
                let texts: Vec<_> = row.iter().map(|s| format!("x={:.0} {:?}", s.x, s.text)).collect();
                println!("  ROW y={:.0}: {}", row[0].y, texts.join(" | "));
            }
        }
    }

    #[test]
    fn test_shringar_not_mixed_with_shivam() {
        if !std::path::Path::new(PDF_PATH).exists() { return; }
        let result = parse();
        let shivam: Vec<_> = result.transactions.iter()
            .filter(|t| t.security_name.to_lowercase().contains("shivam"))
            .collect();
        let shringar: Vec<_> = result.transactions.iter()
            .filter(|t| t.security_name.to_lowercase().contains("shringar"))
            .collect();
        println!("Shivam txns ({}), Shringar txns ({})", shivam.len(), shringar.len());
        assert_eq!(shivam.len(), 4, "Shivam should have exactly 4 txns matching global report");
        assert!(shringar.len() > 0, "Shringar should have transactions");
    }
}
