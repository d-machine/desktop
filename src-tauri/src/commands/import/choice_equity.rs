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

use crate::{commands::import::pdf_utils, db};
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

fn month_num(abbr: &str) -> Option<u32> {
    match abbr.to_lowercase().as_str() {
        "jan" => Some(1),
        "feb" => Some(2),
        "mar" => Some(3),
        "apr" => Some(4),
        "may" => Some(5),
        "jun" => Some(6),
        "jul" => Some(7),
        "aug" => Some(8),
        "sep" => Some(9),
        "oct" => Some(10),
        "nov" => Some(11),
        "dec" => Some(12),
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
    if parts.len() != 3 {
        return None;
    }
    let day: u32 = parts[0].parse().ok()?;
    let month = month_num(parts[1])?;
    let year: u32 = parts[2].parse().ok()?;
    Some(format!("{}-{:02}-{:02}", year, month, day))
}

/// If col0 contains a security name + BSE code immediately followed by a date
/// string (e.g. "TATA STEEL LTD. - 50047009-Jul-2024"), split and return
/// (name_with_bse_code, date_string).  Returns None if no date tail is found.
fn split_col0_date(col0: &str) -> Option<(String, String)> {
    // Look for a date pattern anywhere in col0 that is preceded by a digit
    // (i.e. the last digit of a BSE code runs straight into the date).
    let re = date_re();
    let m = re.find(col0)?;
    // The character immediately before the date match must be a digit (BSE code end)
    let before = &col0[..m.start()];
    if !before.ends_with(|c: char| c.is_ascii_digit()) {
        return None;
    }
    Some((before.trim().to_string(), m.as_str().to_string()))
}

fn parse_qty(s: &str) -> f64 {
    let s = s.trim();
    if s == "-" || s.is_empty() {
        return 0.0;
    }
    s.replace(',', "").trim().parse().unwrap_or(0.0)
}

fn parse_price(s: &str) -> f64 {
    let s = s.trim();
    if s == "-" || s.is_empty() {
        return 0.0;
    }
    s.replace(',', "").trim().parse().unwrap_or(0.0)
}

/// Scan rows for a labelled field, e.g. ["Client ID", ":  D03695", …].
/// Looks for a span whose text matches `key` then takes the very next span,
/// stripping any leading ": " punctuation.
fn extract_header_field(rows: &[Vec<pdf_utils::TextSpan>], key: &str) -> Option<String> {
    let key_lc = key.to_lowercase();
    for row in rows {
        for (i, span) in row.iter().enumerate() {
            if span.text.trim().to_lowercase() == key_lc {
                if let Some(next) = row.get(i + 1) {
                    let val = next.text.trim_start_matches(':').trim().to_string();
                    if !val.is_empty() {
                        return Some(val);
                    }
                }
            }
        }
    }
    None
}

/// Parse a Choice Equity Global Details Report PDF.
#[tauri::command]
pub fn parse_choice_equity_pdf(file_path: String) -> Result<ChoiceEquityParseResult, String> {
    let all_pages = pdf_utils::extract_all_page_spans(&file_path)?;

    // Extract client metadata from the first page header.
    let header_rows: Vec<Vec<pdf_utils::TextSpan>> = all_pages
        .first()
        .map(|page| pdf_utils::page_spans_to_rows(page.clone(), 5.0))
        .unwrap_or_default();
    let client_id   = extract_header_field(&header_rows, "Client ID");
    let client_name = extract_header_field(&header_rows, "Name");

    // ── Step 1: Flatten all pages into a single cell list ────────────────────
    // Each element is a 9-cell row. This lets us look at adjacent rows across
    // page boundaries, which is necessary for merging split logical rows.
    let mut all_cells: Vec<Vec<String>> = all_pages
        .iter()
        .flat_map(|page| {
            pdf_utils::page_spans_to_rows(page.clone(), 5.0)
                .into_iter()
                .filter(|r| !r.is_empty())
                .map(|r| {
                    let mut cells = pdf_utils::spans_to_cells(&r, &EQ_COL_X);
                    cells.resize(9, String::new());
                    // If col0 contains a security name + BSE code immediately followed
                    // by a date string (e.g. "TATA STEEL LTD. - 50047009-Jul-2024"),
                    // the date landed in the wrong column bucket because its x position
                    // was just below the col0/col1 boundary. Split it out into col1.
                    if cells[1].trim().is_empty() {
                        if let Some((name_part, date_part)) = split_col0_date(&cells[0]) {
                            cells[0] = name_part;
                            cells[1] = date_part;
                        }
                    }
                    cells
                })
        })
        .collect();

    // ── Step 2: Merge split rows ─────────────────────────────────────────────
    // When a logical row straddles a page break the PDF yields two physical rows:
    //   row i:   incomplete date (e.g. "19-Nov-202") and/or truncated security name
    //   row i+k: the missing tail (e.g. col1="5", col0="LTD. - 532281")
    // Between them sit header/footer rows with no trade data.
    //
    // Two kinds of incomplete rows need merging:
    //
    //   1. Incomplete section header: col0 has partial name (no BSE code), all other
    //      cols empty. Look ahead for col0 that, when appended, completes the name.
    //      Only check col0 — section headers never have date content.
    //
    //   2. Incomplete data row: has trading data in cols 2+, but col0 is missing the
    //      BSE code suffix and/or col1 has a truncated date. Look ahead for a row that
    //      completes the security name (col0) and/or the date (col1).
    //
    // When scanning forward, page/column header rows are SKIPPED (not used as
    // candidates and not treated as terminators).  Only a genuine data row or a
    // complete section header stops the scan.
    let mut i = 0;
    while i < all_cells.len() {
        let col0 = all_cells[i][0].trim().to_string();
        let col1 = all_cells[i][1].trim().to_string();
        let has_trading = all_cells[i][2..].iter().any(|c| !c.trim().is_empty());
        let all_other_empty = all_cells[i][1..].iter().all(|c| c.trim().is_empty());

        // ── Classify current row ─────────────────────────────────────────────
        // Section header: only col0 has content, all other cols empty.
        let is_section_hdr = !col0.is_empty() && all_other_empty;
        // Data row: has a date field (col1) AND trading data (cols 2+).
        let is_data_row = !col1.is_empty() && has_trading;

        let incomplete_section_hdr = is_section_hdr && !section_re().is_match(&col0);
        let incomplete_data_sec    = is_data_row && !col0.is_empty() && !section_re().is_match(&col0);
        let incomplete_data_date   = is_data_row && parse_equity_date(&col1).is_none();

        if incomplete_section_hdr || incomplete_data_sec || incomplete_data_date {
            let mut found_j = None;

            for j in (i + 1)..all_cells.len().min(i + 20) {
                let next_col0 = all_cells[j][0].trim().to_string();
                let next_col1 = all_cells[j][1].trim().to_string();
                let next_has_trading = all_cells[j][2..].iter().any(|c| !c.trim().is_empty());
                let next_all_other_empty = all_cells[j][1..].iter().all(|c| c.trim().is_empty());

                // ── Stop scanning if we hit a real data row or complete section header ──
                if next_has_trading && !next_col1.is_empty() {
                    break; // genuine data row — no more splitting possible
                }
                if !next_col0.is_empty() && next_all_other_empty && section_re().is_match(&next_col0) {
                    break; // complete section header — boundary between securities
                }

                // ── Skip page/column header rows ──────────────────────────────────────
                // These rows belong to the page layout, not the data, and should
                // never be used as completion candidates.
                let is_layout_row =
                    next_col1.to_uppercase() == "TOTAL"
                    || next_col1.to_lowercase().contains("date")
                    || next_col0 == "Security"
                    || next_col0.starts_with("D0")   // client-ID header "D03695 SUMIT KHAITAN"
                    || (!next_col0.is_empty() && all_cells[j][8].trim().to_lowercase().contains("print"))
                    || all_cells[j][4].trim().to_lowercase().starts_with("global")
                    || all_cells[j][2].trim() == "Buy"
                    || all_cells[j][2].trim() == "Qty";
                if is_layout_row {
                    continue; // skip — keep scanning past it
                }

                // ── Only rows with content in col0 or col1 (and no trading data) are candidates ──
                if next_has_trading || (next_col0.is_empty() && next_col1.is_empty()) {
                    continue;
                }

                // ── Rule 1: section header — only match col0 ─────────────────────────
                if incomplete_section_hdr {
                    if !next_col0.is_empty() {
                        let combined = format!("{} {}", col0, next_col0);
                        if section_re().is_match(combined.trim()) {
                            found_j = Some(j);
                            break;
                        }
                    }
                    continue; // section header completion is only via col0
                }

                // ── Rule 2: data row — match col0 (security) and/or col1 (date) ──────
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
                    found_j = Some(j);
                    break;
                }
            }

            if let Some(j) = found_j {
                let next = all_cells.remove(j);
                if (incomplete_section_hdr || incomplete_data_sec) && !next[0].trim().is_empty() {
                    let combined = format!("{} {}", all_cells[i][0].trim(), next[0].trim());
                    all_cells[i][0] = combined;
                }
                if incomplete_data_date && !next[1].trim().is_empty() {
                    let combined = format!("{}{}", all_cells[i][1].trim(), next[1].trim());
                    all_cells[i][1] = combined;
                }
                // Re-examine the merged row — it may still need another pass.
                continue;
            }
            // No completion found — leave row as-is; surfaced via skipped_details.
        }
        i += 1;
    }

    // ── Step 3: Parse the (now-merged) cell list ─────────────────────────────
    let mut trades: Vec<ParsedEquityTrade> = Vec::new();
    let mut skipped = 0usize;
    let mut intraday = 0usize;
    let mut skipped_details: Vec<SkippedRow> = Vec::new();

    let mut cur_security = String::new();
    let mut cur_bse_code = String::new();

    for cells in all_cells {
        let col0 = cells[0].trim().to_string();
        let col1 = cells[1].trim().to_string();
        let col2 = cells[2].trim().to_string(); // buy qty
        let col3 = cells[3].trim().to_string(); // buy price
        let col4 = cells[4].trim().to_string(); // sell qty
        let col5 = cells[5].trim().to_string(); // sell price

        // ── Skip report header / column header rows ───────────────────────
        if col1 == "Date" || col1.to_lowercase().contains("date") { continue; }
        if col0.to_lowercase().contains("dear")
            || col0.to_lowercase().contains("please")
            || col0.to_lowercase().contains("statement")
        { continue; }

        // ── TOTAL row ─────────────────────────────────────────────────────
        if col1.to_uppercase() == "TOTAL" { continue; }

        // ── Section header ────────────────────────────────────────────────
        let other_empty = cells[1..].iter().all(|c| c.trim().is_empty());
        if other_empty && !col0.is_empty() {
            if let Some(caps) = section_re().captures(&col0) {
                cur_security = caps[1].trim().to_string();
                cur_bse_code = caps[2].trim().to_string();
            } else {
                // Partial/broken section header that the merge pass couldn't fix.
                // Reset cur_security so following rows aren't misattributed to the
                // previous security.
                cur_security.clear();
                cur_bse_code.clear();
            }
            continue;
        }

        // ── Data row ──────────────────────────────────────────────────────
        if cur_security.is_empty() { skipped += 1; continue; }

        let date = match parse_equity_date(&col1) {
            Some(d) => d,
            None => {
                if !col1.is_empty() {
                    skipped_details.push(SkippedRow {
                        security_name: cur_security.clone(),
                        raw_date: col1.clone(),
                        buy_qty: col2.clone(),
                        buy_price: col3.clone(),
                        sell_qty: col4.clone(),
                        sell_price: col5.clone(),
                        reason: format!("Could not parse date \"{}\"", col1),
                    });
                }
                skipped += 1;
                continue;
            }
        };

        let buy_qty   = parse_qty(&col2);
        let buy_price = parse_price(&col3);
        let sell_qty  = parse_qty(&col4);
        let sell_price = parse_price(&col5);

        // Col 6 = net qty, col 7 = net price — fallback when col2/col4 both zero.
        let net_qty_col: f64 = cells.get(6)
            .map(|s| s.trim().replace(',', "").parse().unwrap_or(0.0))
            .unwrap_or(0.0);
        let net_price_col: f64 = cells.get(7).map(|s| parse_price(s)).unwrap_or(0.0);

        let is_pure_buy  = buy_qty > 0.0 && sell_qty == 0.0;
        let is_pure_sell = sell_qty > 0.0 && buy_qty == 0.0;
        let is_intraday  = buy_qty > 0.0 && sell_qty > 0.0;

        if is_pure_buy {
            trades.push(ParsedEquityTrade {
                trade_date: date, security_name: cur_security.clone(),
                exchange_code: cur_bse_code.clone(), txn_type: "BUY".to_string(),
                quantity: buy_qty, price: buy_price, trade_segment: "EQ".to_string(),
            });
        } else if is_pure_sell {
            trades.push(ParsedEquityTrade {
                trade_date: date, security_name: cur_security.clone(),
                exchange_code: cur_bse_code.clone(), txn_type: "SELL".to_string(),
                quantity: sell_qty, price: sell_price, trade_segment: "EQ".to_string(),
            });
        } else if is_intraday {
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
        } else if net_qty_col != 0.0 && net_price_col > 0.0 {
            let (txn_type, qty) = if net_qty_col > 0.0 {
                ("BUY".to_string(), net_qty_col)
            } else {
                ("SELL".to_string(), -net_qty_col)
            };
            trades.push(ParsedEquityTrade {
                trade_date: date, security_name: cur_security.clone(),
                exchange_code: cur_bse_code.clone(), txn_type,
                quantity: qty, price: net_price_col, trade_segment: "EQ".to_string(),
            });
        } else {
            skipped_details.push(SkippedRow {
                security_name: cur_security.clone(),
                raw_date: col1.clone(),
                buy_qty: col2.clone(), buy_price: col3.clone(),
                sell_qty: col4.clone(), sell_price: col5.clone(),
                reason: "Both buy and sell quantities are zero".to_string(),
            });
            skipped += 1;
        }
    }

    Ok(ChoiceEquityParseResult {
        total_rows: trades.len(),
        skipped_rows: skipped,
        intraday_rows: intraday,
        transactions: trades,
        client_id,
        client_name,
        skipped_details,
    })
}

// ─── Import ───────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct ChoiceEquityImportResult {
    pub imported: usize,
    pub skipped: usize,
    pub auto_created_instruments: usize,
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
    let mut imported = 0usize;
    let mut skipped = 0usize;
    let mut auto_created = 0usize;

    let equity_type_id: i64 = conn
        .query_row(
            "SELECT instrument_type_id FROM instrument_types WHERE name='EQUITY' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(1);

    let bse_exchange_id: Option<i64> = conn
        .query_row(
            "SELECT exchange_id FROM exchanges WHERE code='BSE' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .ok();

    for trade in &transactions {
        // Resolve instrument by BSE code
        let mut instrument_id: Option<i64> = conn
            .query_row(
                "SELECT i.instrument_id FROM instruments i
                 JOIN instrument_equity ie ON i.instrument_id = ie.instrument_id
                 WHERE ie.bse_code = ?1 LIMIT 1",
                [&trade.exchange_code],
                |row| row.get(0),
            )
            .ok();

        // Auto-create if not found
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
                    [&trade.security_name],
                    |row| row.get(0),
                )
                .ok();

            if let Some(id) = new_id {
                // Insert BSE symbol into instrument_equity
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
                continue;
            }
        };

        let price_paise = (trade.price * 100.0).round() as i64;

        // Deduplicate — quantity is included to allow two distinct same-day
        // same-price trades of different sizes to coexist.
        let exists: bool = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions
                 WHERE account_id=?1 AND instrument_id=?2 AND trade_date=?3
                   AND price_paise=?4 AND quantity=?5 AND txn_type=?6 AND trade_segment=?7",
                rusqlite::params![
                    account_id,
                    instrument_id,
                    trade.trade_date,
                    price_paise,
                    trade.quantity,
                    trade.txn_type,
                    trade.trade_segment,
                ],
                |row| row.get::<_, i64>(0),
            )
            .map(|c| c > 0)
            .unwrap_or(false);

        if exists {
            skipped += 1;
            continue;
        }

        let qty = trade.quantity;
        let gross_paise = (qty * trade.price * 100.0).round() as i64;
        let total_value_paise = if trade.txn_type == "BUY" {
            -gross_paise
        } else {
            gross_paise
        };

        conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, txn_type, trade_segment, trade_date,
                 quantity, price_paise, brokerage_paise, stt_paise, other_charges_paise,
                 total_value_paise, notes)
             VALUES (?1,?2,?3,?4,?5,?6,?7,0,0,0,?8,NULL)",
            rusqlite::params![
                account_id,
                instrument_id,
                trade.txn_type,
                trade.trade_segment,
                trade.trade_date,
                qty,
                price_paise,
                total_value_paise,
            ],
        )
        .map_err(|e| e.to_string())?;

        imported += 1;
    }

    Ok(ChoiceEquityImportResult {
        imported,
        skipped,
        auto_created_instruments: auto_created,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_parse_equity_pdf() {
        let path = "/home/dmachine/workspace/D03695_Global_Details_Report_202604131819042 (Copy).pdf";
        if !std::path::Path::new(path).exists() {
            return;
        }
        let result = parse_choice_equity_pdf(path.to_string()).expect("parse failed");
        println!("Equity Total: {}, Skipped: {}, Intraday: {}",
            result.total_rows, result.skipped_rows, result.intraday_rows);
        for t in result.transactions.iter().take(5) {
            println!("  {} | {} | {} | {} qty={} price={}",
                t.trade_date, t.txn_type, t.security_name,
                t.exchange_code, t.quantity, t.price);
        }
        assert!(result.total_rows > 0, "expected trades, got 0");
        for t in &result.transactions {
            assert!(!t.trade_date.is_empty(), "empty trade date");
            assert!(t.quantity > 0.0, "zero quantity for {}", t.security_name);
            assert!(t.price > 0.0, "zero price for {}", t.security_name);
        }
    }
}

#[cfg(test)]
mod tests2 {
    use super::*;
    #[test]
    fn test_hcl_transactions() {
        let path = "/home/dmachine/workspace/D03695_Global_Details_Report_202604131819042 (Copy).pdf";
        if !std::path::Path::new(path).exists() { return; }
        let result = parse_choice_equity_pdf(path.to_string()).expect("parse failed");
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
        let path = "/home/dmachine/workspace/D03695_Global_Details_Report_202604131819042 (Copy).pdf";
        if !std::path::Path::new(path).exists() { return; }
        let result = parse_choice_equity_pdf(path.to_string()).expect("parse failed");
        let tata: Vec<_> = result.transactions.iter()
            .filter(|t| t.security_name.to_lowercase().contains("tata steel"))
            .collect();
        println!("Tata Steel parsed ({}):", tata.len());
        for t in &tata {
            println!("  {} | {} | {} | {} @ {}", t.trade_date, t.txn_type, t.trade_segment, t.quantity, t.price);
        }
        let skipped: Vec<_> = result.skipped_details.iter()
            .filter(|r| r.security_name.to_lowercase().contains("tata steel"))
            .collect();
        println!("Tata Steel skipped ({}):", skipped.len());
        for r in &skipped {
            println!("  sec={} date={} reason={}", r.security_name, r.raw_date, r.reason);
        }

        // Dump raw cells around "tata steel" BEFORE the merge pass
        let all_pages: Vec<Vec<pdf_utils::TextSpan>> = pdf_utils::extract_all_page_spans(path).expect("extract failed");
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
            if row.iter().any(|c: &String| c.to_lowercase().contains("tata steel")) {
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
    fn test_shringar_section_header() {
        let path = "/home/dmachine/workspace/D03695_Global_Details_Report_202604131819042 (Copy).pdf";
        if !std::path::Path::new(path).exists() { return; }
        let result = parse_choice_equity_pdf(path.to_string()).expect("parse failed");
        let shivam: Vec<_> = result.transactions.iter()
            .filter(|t| t.security_name.to_lowercase().contains("shivam"))
            .collect();
        let shringar: Vec<_> = result.transactions.iter()
            .filter(|t| t.security_name.to_lowercase().contains("shringar"))
            .collect();
        println!("Shivam txns ({})", shivam.len());
        println!("Shringar txns ({})", shringar.len());
        assert_eq!(shivam.len(), 4, "Shivam should have exactly 4 txns matching global report");
        assert!(shringar.len() > 0, "Shringar should have transactions");
    }
}
