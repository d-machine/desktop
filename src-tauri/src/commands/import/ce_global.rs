//! Parser for Choice Equity Broking — Global Details Report PDF (border-based).
//!
//! PDF border V-lines define 9 columns (consistent across all data pages):
//!   Col 0 (x≈ 20): Security name + BSE code
//!   Col 1 (x≈127): Date (DD-Mon-YYYY)
//!   Col 2 (x≈180): Buy Qty
//!   Col 3 (x≈233): Buy Price
//!   Col 4 (x≈287): Sell Qty
//!   Col 5 (x≈340): Sell Price
//!   Col 6 (x≈393): Net Qty
//!   Col 7 (x≈447): Net Price
//!   Col 8 (x≈500): Net Value
//!
//! The border-based grid replaces the old Y-tolerance row grouper, eliminating
//! the need for `merge_split_rows`, `fix_col0_date_bleed`, and `is_layout_row`.

use crate::{commands::import::{common, pdf_utils, flag_oversells}, db};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ─── Extraction constants ─────────────────────────────────────────────────────

const X_GAP:        f32 = 6.0;
// Slightly looser than the default 2.0 so that year digits placed 3pt below
// the rest of the date string (a common PDF rendering artifact) are merged
// into a single span, while the 4.6pt gap between the 3-line layout's logical
// sub-rows (security name / data / BSE code) stays intact.
const CHAR_Y_TOL:   f32 = 3.5;
const MIN_H_LEN:    f32 = 20.0;
const MIN_V_LEN:    f32 = 10.0;
const CLUSTER_GAP:  f32 = 3.0;
const COL_SNAP:     f32 = 2.0;
// Shift column X boundaries rightward by this amount after deriving them from
// V-lines. Handles security-name text that wraps just past the col 0/col 1
// V-line (e.g. "…Lim" ends at x=126.67 and "ited" starts at x=127 — without
// the shift it would land in col 1 instead of col 0).
const COL_PADDING:  f32 = 4.0;

// ─── Column indices (from V-line order on data pages) ─────────────────────────

const COL_SECURITY: usize = 0;
const COL_DATE:     usize = 1;
const COL_BUY_QTY:  usize = 2;
const COL_BUY_PX:   usize = 3;
const COL_SELL_QTY: usize = 4;
const COL_SELL_PX:  usize = 5;
const COL_NET_VAL: usize = 8;
// A page must have at least this many columns to be treated as a data page.
const MIN_COLS: usize = 6; // through COL_SELL_PX

// ─── Regexes ─────────────────────────────────────────────────────────────────

fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\d{2}-[A-Za-z]{3}-\d{4}").unwrap())
}

fn section_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(.+?)\s*-\s*(\d+)\s*$").unwrap())
}

fn statement_date_range_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        Regex::new(r"(?i)from\s+(\d{2}/\d{2}/\d{4})\s+[Tt]o\s+(\d{2}/\d{2}/\d{4})").unwrap()
    })
}

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedCharge {
    pub charge_type:  String,
    pub amount_paise: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedEquityTrade {
    pub trade_date:    String,
    pub security_name: String,
    pub exchange_code: String,
    pub txn_type:      String,
    pub quantity:      f64,
    pub price:         f64,
    pub trade_segment: String,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct SkippedRow {
    pub security_name: String,
    pub raw_date:      String,
    pub buy_qty:       String,
    pub buy_price:     String,
    pub sell_qty:      String,
    pub sell_price:    String,
    pub reason:        String,
}

#[derive(Debug, Serialize)]
pub struct CeGlobalParseResult {
    pub transactions:    Vec<ParsedEquityTrade>,
    pub total_rows:      usize,
    pub skipped_rows:    usize,
    pub intraday_rows:   usize,
    pub client_id:       Option<String>,
    pub client_name:     Option<String>,
    pub skipped_details: Vec<SkippedRow>,
    pub charges:         Vec<ParsedCharge>,
    pub statement_start: Option<String>,
    pub statement_end:   Option<String>,
    pub pages_scanned:   usize,
    pub pages_with_grid: usize,
}

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
pub struct CeGlobalImportResult {
    pub imported:                 usize,
    pub skipped:                  usize,
    pub auto_created_instruments: usize,
    pub skipped_details:          Vec<SkippedDetail>,
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

fn parse_equity_date(raw: &str) -> Option<String> {
    let clean: String = raw.split_whitespace().collect();
    let m = date_re().find(&clean)?;
    let parts: Vec<&str> = m.as_str().splitn(3, '-').collect();
    if parts.len() != 3 { return None; }
    let day:   u32 = parts[0].parse().ok()?;
    let month       = month_num(parts[1])?;
    let year:  u32 = parts[2].parse().ok()?;
    Some(format!("{year}-{month:02}-{day:02}"))
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

fn parse_date_dmyslash(s: &str) -> Option<String> {
    let parts: Vec<&str> = s.splitn(3, '/').collect();
    if parts.len() != 3 { return None; }
    let day:   u32 = parts[0].parse().ok()?;
    let month: u32 = parts[1].parse().ok()?;
    let year:  u32 = parts[2].parse().ok()?;
    if day < 1 || day > 31 || month < 1 || month > 12 { return None; }
    Some(format!("{year}-{month:02}-{day:02}"))
}

fn make_trade(
    date: &str, security: &str, bse_code: &str,
    txn_type: &str, qty: f64, price: f64, segment: &str,
) -> ParsedEquityTrade {
    ParsedEquityTrade {
        trade_date:    date.to_string(),
        security_name: security.to_string(),
        exchange_code: bse_code.to_string(),
        txn_type:      txn_type.to_string(),
        quantity:      qty,
        price,
        trade_segment: segment.to_string(),
    }
}

// ─── Client metadata extraction ───────────────────────────────────────────────

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

fn extract_client_metadata(all_pages: &[Vec<pdf_utils::TextSpan>]) -> (Option<String>, Option<String>) {
    let header_rows: Vec<Vec<pdf_utils::TextSpan>> = all_pages
        .first()
        .map(|page| pdf_utils::page_spans_to_rows(page.clone(), 5.0))
        .unwrap_or_default();
    let client_id   = extract_header_field(&header_rows, "Client ID");
    let client_name = extract_header_field(&header_rows, "Name");
    (client_id, client_name)
}

fn extract_statement_date_range(all_pages: &[Vec<pdf_utils::TextSpan>]) -> (Option<String>, Option<String>) {
    let re = statement_date_range_re();
    for page in all_pages.iter().take(2) {
        let full_text: String = page.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
        if let Some(caps) = re.captures(&full_text) {
            return (parse_date_dmyslash(&caps[1]), parse_date_dmyslash(&caps[2]));
        }
    }
    (None, None)
}

// ─── Core parser ─────────────────────────────────────────────────────────────

// ─── Cross-page row merging ───────────────────────────────────────────────────

/// Holds the raw cell strings of a data row whose date could not be fully parsed —
/// typically because the row spans a page break (e.g. date = "13-Aug-202",
/// continuation = "4" on the next page).
struct PartialRow {
    security:  String,
    bse_code:  String,
    date:      String,
    buy_qty:   String,
    buy_px:    String,
    sell_qty:  String,
    sell_px:   String,
}

fn partial_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches a date whose year digit(s) are cut off: DD-Mon-YY{1,3}
    RE.get_or_init(|| Regex::new(r"^\d{2}-[A-Za-z]{3}-\d{1,3}$").unwrap())
}

/// Returns true if `name` is a bare company-name suffix that arrived as a
/// cross-page continuation fragment (e.g. "LTD.", "Limited", "ited").
/// Such fragments should not be accepted as a full security name; instead
/// they are merged with the pending partial_header from the previous page.
fn is_name_fragment(name: &str) -> bool {
    let n = name.trim().to_lowercase();
    matches!(n.as_str(), "limited" | "ltd." | "ltd" | "lim" | "ited" | "ite" | "it" | "d.")
}

/// Try to complete `pend` with continuation cell strings from the next row.
/// Returns `Some((date, buy_qty, buy_px, sell_qty, sell_px))` if the concat
/// of every column pair passes its validation.
fn try_merge(
    pend:          &PartialRow,
    cont_date:     &str,
    cont_buy_qty:  &str,
    cont_buy_px:   &str,
    cont_sell_qty: &str,
    cont_sell_px:  &str,
) -> Option<(String, f64, f64, f64, f64)> {
    let merged_date = format!("{}{}", pend.date.trim(), cont_date.trim());
    let trade_date  = parse_equity_date(&merged_date)?;

    // Concatenate numeric columns; the PDF may split a value like "55." + "50".
    let buy_qty  = parse_qty  (&format!("{}{}", pend.buy_qty.trim(),  cont_buy_qty.trim()));
    let buy_px   = parse_price(&format!("{}{}", pend.buy_px.trim(),   cont_buy_px.trim()));
    let sell_qty = parse_qty  (&format!("{}{}", pend.sell_qty.trim(), cont_sell_qty.trim()));
    let sell_px  = parse_price(&format!("{}{}", pend.sell_px.trim(),  cont_sell_px.trim()));

    Some((trade_date, buy_qty, buy_px, sell_qty, sell_px))
}

// ─── Trade emitter (avoids repeating the if/else block) ──────────────────────

fn emit_trades(
    date: &str, security: &str, bse_code: &str,
    buy_qty: f64, buy_px: f64, sell_qty: f64, sell_px: f64,
    trades: &mut Vec<ParsedEquityTrade>,
    intraday: &mut usize,
    skipped: &mut usize,
    skipped_details: &mut Vec<SkippedRow>,
) {
    if buy_qty > 0.0 && sell_qty == 0.0 {
        trades.push(make_trade(date, security, bse_code, "BUY",  buy_qty,  buy_px,  "EQ"));
    } else if sell_qty > 0.0 && buy_qty == 0.0 {
        trades.push(make_trade(date, security, bse_code, "SELL", sell_qty, sell_px, "EQ"));
    } else if buy_qty > 0.0 && sell_qty > 0.0 {
        *intraday += 1;
        trades.push(make_trade(date, security, bse_code, "BUY",  buy_qty,  buy_px,  "INTRADAY"));
        trades.push(make_trade(date, security, bse_code, "SELL", sell_qty, sell_px, "INTRADAY"));
    } else {
        skipped_details.push(SkippedRow {
            security_name: security.to_string(),
            raw_date:      date.to_string(),
            buy_qty:       format!("{buy_qty}"),
            buy_price:     format!("{buy_px}"),
            sell_qty:      format!("{sell_qty}"),
            sell_price:    format!("{sell_px}"),
            reason:        "Both buy and sell quantities are zero".to_string(),
        });
        *skipped += 1;
    }
}

// ─── Core parser ─────────────────────────────────────────────────────────────

struct ParseOutput {
    trades:          Vec<ParsedEquityTrade>,
    skipped:         usize,
    intraday:        usize,
    skipped_details: Vec<SkippedRow>,
    #[allow(dead_code)] // populated for test cross-checks; not read in production code
    pdf_totals:      std::collections::HashMap<String, (f64, f64)>, // security → (buy_qty, sell_qty)
    charges:         Vec<ParsedCharge>,
    pages_scanned:   usize,
    pages_with_grid: usize,
}

fn parse_pdf(doc: &lopdf::Document, all_page_spans: &[Vec<pdf_utils::TextSpan>]) -> ParseOutput {
    let mut trades:          Vec<ParsedEquityTrade>                   = Vec::new();
    let mut skipped          = 0usize;
    let mut intraday         = 0usize;
    let mut skipped_details: Vec<SkippedRow>                         = Vec::new();
    let mut pdf_totals:      std::collections::HashMap<String,(f64,f64)> = Default::default();
    let mut charge_accum:    std::collections::HashMap<String, i64>  = Default::default();

    let mut pages_scanned   = 0usize;
    let mut pages_with_grid = 0usize;
    let mut cur_security    = String::new();
    let mut cur_bse_code    = String::new();
    let mut in_zz_expenses  = false;
    let mut partial:        Option<PartialRow> = None;
    let mut partial_header: Option<String>     = None; // incomplete section-header text from prev page

    let mut page_nums: Vec<u32> = doc.get_pages().keys().copied().collect();
    page_nums.sort();

    for (page_idx, &page_num) in page_nums.iter().enumerate() {
        let page_spans = match all_page_spans.get(page_idx) {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };

        pages_scanned += 1;
        let lines = pdf_utils::extract_page_lines(doc, page_num);
        let (row_ys, col_xs) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);

        // Skip pages without enough columns to contain trading data.
        if col_xs.len() <= MIN_COLS { continue; }
        pages_with_grid += 1;

        // Shift column boundaries right by COL_PADDING so that security-name
        // text overflowing just past the V-line stays in col 0.
        let padded_col_xs: Vec<f32> = col_xs.iter().map(|&x| x + COL_PADDING).collect();
        let cells  = pdf_utils::build_cell_map(page_spans, &row_ys, &padded_col_xs, COL_SNAP);
        let n_rows = row_ys.len() + 1;

        // Iterate top-to-bottom (highest row index = highest Y = top of page)
        // so that section headers are processed before their data rows and TOTAL.
        for row in (0..n_rows).rev() {
            let sec  = pdf_utils::cell_text(&cells, row, COL_SECURITY);
            let date = pdf_utils::cell_text(&cells, row, COL_DATE);

            if sec.is_empty() && date.is_empty() { continue; }

            let date_t  = date.trim();
            let date_lc = date_t.to_lowercase();

            // Layout / column-header rows — skip but preserve partial state.
            if date_lc == "date" || date_lc.contains("buy") || date_lc.contains("sell")
                || date_lc.contains("qty") || date_lc.contains("price") { continue; }
            if date_t.to_uppercase() == "TOTAL" {
                let t_bq = parse_qty  (&pdf_utils::cell_text(&cells, row, COL_BUY_QTY));
                let t_sq = parse_qty  (&pdf_utils::cell_text(&cells, row, COL_SELL_QTY));
                pdf_totals.insert(cur_security.clone(), (t_bq, t_sq));
                continue;
            }

            // Section header: only col 0 has content; must match "Name - BSECode".
            //
            // Failure modes handled:
            //  A) Page-title / footer rows — row n_rows-1 (title above table) and
            //     row 0 (footer "D03695 SUMIT KHAITAN…") must not pollute
            //     partial_header.  Skip both for section detection.
            //  B) Cross-page col-0 split — "TILAKNAGAR INDUSTRIES" on page N +
            //     "LTD. - 123456" on page N+1.  Fragment names are detected by
            //     is_name_fragment() and merged via partial_header.
            //  C) Same-row column split — security name overflows into col 1, e.g.
            //     col0="Indian Energy Exchange Lim", col1="ited - 540750".
            //     Detected when date_t looks like a name suffix rather than a date.

            // C) Column-split header: date col contains a name+code suffix, e.g.
            //    col0="Indian Energy Exchange Lim", col1="ited - 540750".
            //    Distinguish from real dates: the name part before " - " has no digits.
            let col1_is_name_suffix = !date_t.is_empty()
                && !sec.is_empty()
                && section_re().captures(date_t)
                    .map(|caps| !caps[1].chars().any(|c| c.is_ascii_digit()))
                    .unwrap_or(false);

            let (effective_sec, effective_date_empty) = if col1_is_name_suffix {
                (format!("{} {}", sec.trim(), date_t.trim()), true)
            } else {
                (sec.clone(), date_t.is_empty())
            };

            if effective_date_empty && !effective_sec.is_empty() {
                // A) Page-title and footer rows — skip for section detection.
                if row == n_rows - 1 || row == 0 { continue; }

                // ZZ-Expenses section — switch to expense-row mode.
                if effective_sec.trim() == "ZZ-Expenses" {
                    in_zz_expenses = true;
                    cur_security   = String::new();
                    cur_bse_code   = String::new();
                    partial_header = None;
                    partial        = None;
                    continue;
                }

                // 1. Try a direct match (no prefix).  Accept only if the name
                //    part is not a bare suffix fragment like "LTD." or "Limited".
                if let Some(caps) = section_re().captures(effective_sec.trim()) {
                    if !is_name_fragment(&caps[1]) {
                        cur_security   = caps[1].trim().to_string();
                        cur_bse_code   = caps[2].trim().to_string();
                        in_zz_expenses = false;
                        partial_header = None;
                        partial        = None;
                        continue;
                    }
                }

                // 2. Try prepending the saved cross-page fragment.
                let candidate = if let Some(ref ph) = partial_header {
                    format!("{} {}", ph.trim(), effective_sec.trim())
                } else {
                    effective_sec.trim().to_string()
                };

                if let Some(caps) = section_re().captures(&candidate) {
                    if !is_name_fragment(&caps[1]) {
                        cur_security   = caps[1].trim().to_string();
                        cur_bse_code   = caps[2].trim().to_string();
                        partial_header = None;
                        partial        = None;
                    } else {
                        partial_header = Some(candidate);
                    }
                } else {
                    // Pure digit string (e.g. "532735") = BSE-code stub from a
                    // cross-page data-row split; keep cur_security so the TOTAL is
                    // attributed correctly. Anything else (options headers, expense
                    // sections, GRAND TOTAL) = reset cur_security.
                    let is_code_stub = effective_sec.trim()
                        .chars().all(|c| c.is_ascii_digit() || c.is_whitespace());
                    if !is_code_stub {
                        cur_security = String::new();
                        cur_bse_code = String::new();
                    }
                    partial_header = Some(effective_sec.trim().to_string());
                }
                continue;
            }

            // ZZ-Expenses data rows: col0=charge name, date contains "Invalid"
            if in_zz_expenses && date_lc.contains("invalid") {
                let charge_name = sec.trim().to_uppercase();
                let raw_val = pdf_utils::cell_text(&cells, row, COL_NET_VAL);
                let amount  = parse_price(&raw_val).abs();
                if amount > 0.0 {
                    let mapped = if charge_name.contains("CGST") || charge_name.contains("SGST") {
                        "GST"
                    } else if charge_name.contains("STT") {
                        "STT"
                    } else if charge_name.contains("STAMP") {
                        "STAMP_DUTY"
                    } else {
                        "OTHER"
                    };
                    let paise = (amount * 100.0).round() as i64;
                    *charge_accum.entry(mapped.to_string()).or_insert(0i64) += paise;
                }
                continue;
            }

            if cur_security.is_empty() { continue; }

            // Collect this row's raw numeric cells once.
            let raw_bq  = pdf_utils::cell_text(&cells, row, COL_BUY_QTY);
            let raw_bp  = pdf_utils::cell_text(&cells, row, COL_BUY_PX);
            let raw_sq  = pdf_utils::cell_text(&cells, row, COL_SELL_QTY);
            let raw_sp  = pdf_utils::cell_text(&cells, row, COL_SELL_PX);

            // ── Try completing a pending cross-page partial row ────────────────
            if let Some(pend) = partial.take() {
                if let Some((td, bq, bp, sq, sp)) =
                    try_merge(&pend, date_t, &raw_bq, &raw_bp, &raw_sq, &raw_sp)
                {
                    emit_trades(&td, &pend.security, &pend.bse_code,
                                bq, bp, sq, sp,
                                &mut trades, &mut intraday,
                                &mut skipped, &mut skipped_details);
                    continue;
                }
                // Can't merge — the partial was genuinely invalid; report it.
                skipped_details.push(SkippedRow {
                    security_name: pend.security.clone(),
                    raw_date:      pend.date.clone(),
                    buy_qty:       pend.buy_qty.clone(),
                    buy_price:     pend.buy_px.clone(),
                    sell_qty:      pend.sell_qty.clone(),
                    sell_price:    pend.sell_px.clone(),
                    reason:        format!("Could not parse date {:?}", pend.date),
                });
                skipped += 1;
                // Fall through and process the current row normally.
            }

            // ── Data row: parse date ──────────────────────────────────────────
            let trade_date = match parse_equity_date(date_t) {
                Some(d) => d,
                None => {
                    if !date_t.is_empty() {
                        // Save as pending only if it looks like a truncated date.
                        if partial_date_re().is_match(date_t) {
                            partial = Some(PartialRow {
                                security: cur_security.clone(),
                                bse_code: cur_bse_code.clone(),
                                date:     date_t.to_string(),
                                buy_qty:  raw_bq,
                                buy_px:   raw_bp,
                                sell_qty: raw_sq,
                                sell_px:  raw_sp,
                            });
                        } else {
                            skipped_details.push(SkippedRow {
                                security_name: cur_security.clone(),
                                raw_date:      date_t.to_string(),
                                buy_qty:       raw_bq,
                                buy_price:     raw_bp,
                                sell_qty:      raw_sq,
                                sell_price:    raw_sp,
                                reason:        format!("Could not parse date {:?}", date_t),
                            });
                            skipped += 1;
                        }
                    }
                    continue;
                }
            };

            emit_trades(&trade_date, &cur_security, &cur_bse_code,
                        parse_qty(&raw_bq), parse_price(&raw_bp),
                        parse_qty(&raw_sq), parse_price(&raw_sp),
                        &mut trades, &mut intraday,
                        &mut skipped, &mut skipped_details);
        }
    }

    // Any leftover partial that was never completed is genuinely unresolvable.
    if let Some(pend) = partial {
        skipped_details.push(SkippedRow {
            security_name: pend.security,
            raw_date:      pend.date.clone(),
            buy_qty:       pend.buy_qty,
            buy_price:     pend.buy_px,
            sell_qty:      pend.sell_qty,
            sell_price:    pend.sell_px,
            reason:        format!("Could not parse date {:?} (no continuation found)", pend.date),
        });
        skipped += 1;
    }

    let charges: Vec<ParsedCharge> = charge_accum.into_iter()
        .filter(|(_, p)| *p > 0)
        .map(|(ct, p)| ParsedCharge { charge_type: ct, amount_paise: p })
        .collect();

    ParseOutput { trades, skipped, intraday, skipped_details, pdf_totals, charges, pages_scanned, pages_with_grid }
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn parse_ce_global_pdf(file_path: String, password: Option<String>) -> Result<CeGlobalParseResult, String> {
    let doc = pdf_utils::load_pdf(&file_path, password.as_deref())?;
    let all_page_spans = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], X_GAP, CHAR_Y_TOL)?;

    let (client_id, client_name)       = extract_client_metadata(&all_page_spans);
    let (statement_start, statement_end) = extract_statement_date_range(&all_page_spans);
    let out = parse_pdf(&doc, &all_page_spans);

    Ok(CeGlobalParseResult {
        total_rows:      out.trades.len(),
        skipped_rows:    out.skipped,
        intraday_rows:   out.intraday,
        transactions:    out.trades,
        skipped_details: out.skipped_details,
        charges:         out.charges,
        client_id,
        client_name,
        statement_start,
        statement_end,
        pages_scanned:   out.pages_scanned,
        pages_with_grid: out.pages_with_grid,
    })
}

#[tauri::command]
pub fn import_ce_global_trades(
    app:             tauri::AppHandle,
    account_id:      i64,
    transactions:    Vec<ParsedEquityTrade>,
    file_paths:      Option<Vec<String>>,
    charges:         Vec<ParsedCharge>,
    statement_start: Option<String>,
    statement_end:   Option<String>,
) -> Result<CeGlobalImportResult, String> {
    let conn = db::acquire()?;
    let mut imported        = 0usize;
    let mut skipped         = 0usize;
    let mut auto_created    = 0usize;
    let mut skipped_details: Vec<SkippedDetail> = Vec::new();

    // Determine ref_no from the first file name (for idempotent batch creation).
    let ref_no: String = file_paths.as_ref()
        .and_then(|ps| ps.first())
        .and_then(|p| std::path::Path::new(p).file_name())
        .map(|n| n.to_string_lossy().into_owned())
        .unwrap_or_else(|| format!("CE-GLOBAL-{}", chrono::Utc::now().timestamp()));

    // One batch per PDF — idempotent: reuse existing batch if already imported.
    let existing_batch_id: Option<i64> = conn.query_row(
        "SELECT batch_id FROM import_batches
         WHERE account_id=?1 AND source_type='CE_GLOBAL' AND ref_no=?2",
        rusqlite::params![account_id, ref_no],
        |r| r.get(0),
    ).ok();
    let newly_created = existing_batch_id.is_none();

    let batch_id = if let Some(id) = existing_batch_id {
        id
    } else {
        conn.execute(
            "INSERT INTO import_batches
                (account_id, source_type, ref_no, broker)
             VALUES (?1, 'CE_GLOBAL', ?2, 'Choice Equity Broking')",
            rusqlite::params![account_id, ref_no],
        ).map_err(|e| e.to_string())?;
        conn.last_insert_rowid()
    };

    for trade in &transactions {
        let (instrument_id, pending_instrument_id) = match common::resolve_equity(
            &conn,
            &trade.security_name,
            None,
            Some(&trade.exchange_code), // exchange_code is the BSE scrip code
            None,
            Some("BSE"),
            &mut auto_created,
        ) {
            Some(pair) => pair,
            None => {
                skipped += 1;
                skipped_details.push(SkippedDetail {
                    trade_date:    trade.trade_date.clone(),
                    security_name: trade.security_name.clone(),
                    txn_type:      trade.txn_type.clone(),
                    quantity:      trade.quantity,
                    price:         trade.price,
                    reason:        "Could not stage instrument".to_string(),
                });
                continue;
            }
        };

        let price_paise = (trade.price * 100.0).round() as i64;
        // Deterministic broker_ref for idempotent reimport dedup.
        // Uses qty in milli-units to avoid float formatting issues.
        let qty_milli   = (trade.quantity * 1000.0).round() as i64;
        let broker_ref  = format!(
            "CE-{}-{}-{}-{}-{}",
            trade.exchange_code, trade.trade_date, trade.txn_type, qty_milli, price_paise,
        );

        let qty               = trade.quantity;
        let gross_paise       = (qty * trade.price * 100.0).round() as i64;
        let total_value_paise = if trade.txn_type == "BUY" { -gross_paise } else { gross_paise };

        let rows = conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, pending_instrument_id, txn_type, trade_segment,
                 trade_date, quantity, price_paise, brokerage_paise, stt_paise,
                 other_charges_paise, total_value_paise, notes, broker_ref, batch_id)
             VALUES (?1,?2,?3,?4,?5,?6,?7,?8,0,0,0,?9,NULL,?10,?11)
             ON CONFLICT(account_id, broker_ref) WHERE broker_ref IS NOT NULL DO UPDATE SET
                 instrument_id         = excluded.instrument_id,
                 pending_instrument_id = excluded.pending_instrument_id,
                 batch_id              = excluded.batch_id
             WHERE transactions.instrument_id         IS NOT excluded.instrument_id
                OR transactions.pending_instrument_id IS NOT excluded.pending_instrument_id",
            rusqlite::params![
                account_id, instrument_id, pending_instrument_id,
                trade.txn_type, trade.trade_segment, trade.trade_date,
                qty, price_paise, total_value_paise, broker_ref, batch_id,
            ],
        ).map_err(|e| e.to_string())?;

        if rows > 0 {
            imported += 1;
        } else {
            skipped += 1;
            skipped_details.push(SkippedDetail {
                trade_date:    trade.trade_date.clone(),
                security_name: trade.security_name.clone(),
                txn_type:      trade.txn_type.clone(),
                quantity:      trade.quantity,
                price:         trade.price,
                reason:        "Duplicate".to_string(),
            });
        }
    }

    // Insert charges for new batches only — re-import is idempotent.
    if newly_created && !charges.is_empty() {
        if let Some(ref start) = statement_start {
            let end = statement_end.as_deref().unwrap_or(start.as_str());
            for charge in &charges {
                if charge.amount_paise <= 0 { continue; }
                let _ = conn.execute(
                    "INSERT INTO charges
                        (account_id, start_date, end_date, charge_type, amount_paise, source, import_batch_id)
                     VALUES (?1, ?2, ?3, ?4, ?5, 'IMPORT', ?6)",
                    rusqlite::params![
                        account_id, start, end,
                        charge.charge_type, charge.amount_paise, batch_id,
                    ],
                );
            }
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

    Ok(CeGlobalImportResult { imported, skipped, auto_created_instruments: auto_created, skipped_details })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const WIN_PDF: &str =
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\D03695_Global_Details_Report_202604131819042.pdf";

    #[test]
    fn test_parse_win() {
        if !std::path::Path::new(WIN_PDF).exists() {
            println!("SKIP: {WIN_PDF} not found");
            return;
        }
        let result = parse_ce_global_pdf(WIN_PDF.to_string(), None).expect("parse failed");
        println!("Total: {}  Skipped: {}  Intraday: {}  client={:?}  name={:?}",
            result.total_rows, result.skipped_rows, result.intraday_rows,
            result.client_id, result.client_name);
        for t in &result.transactions {
            println!("  {} {:6} {:8} qty={:>8.2} @ {:>10.4}  {}",
                t.trade_date, t.txn_type, t.trade_segment,
                t.quantity, t.price, t.security_name);
        }
        if !result.skipped_details.is_empty() {
            println!("\nSKIPPED ({}):", result.skipped_details.len());
            for s in &result.skipped_details {
                println!("  {:?}  date={:?}  reason={}", s.security_name, s.raw_date, s.reason);
            }
        }
        assert!(result.total_rows > 0, "expected transactions");
        assert_eq!(result.skipped_rows, 0, "expected zero skipped rows with border-based parser");
    }

    /// Verifies correctness by comparing per-security trade totals against the PDF's own TOTAL rows.
    /// Both the parsed trades AND the PDF totals come from the same parse_pdf call, so any
    /// systematic section-header attribution is shared — the comparison is internally consistent.
    #[test]
    fn test_totals_win() {
        if !std::path::Path::new(WIN_PDF).exists() {
            println!("SKIP: {WIN_PDF} not found");
            return;
        }

        let doc = lopdf::Document::load(WIN_PDF).expect("load PDF");
        let all_page_spans = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], X_GAP, CHAR_Y_TOL)
            .expect("extract spans");

        let out = parse_pdf(&doc, &all_page_spans);

        // Aggregate parsed trade quantities per security.
        let mut trade_buy:  std::collections::HashMap<String, f64> = Default::default();
        let mut trade_sell: std::collections::HashMap<String, f64> = Default::default();
        for t in &out.trades {
            match t.txn_type.as_str() {
                "BUY"  => *trade_buy .entry(t.security_name.clone()).or_default() += t.quantity,
                "SELL" => *trade_sell.entry(t.security_name.clone()).or_default() += t.quantity,
                _      => {}
            }
        }

        // Print comparison table.
        let mut securities: Vec<String> = out.pdf_totals.keys().cloned().collect();
        securities.sort();

        println!("\n{:<40}  {:>10}  {:>10}  {:>10}  {:>10}  {}",
            "Security", "PDF BuyQty", "Parsed Buy", "PDF SellQty", "Parsed Sell", "Status");
        println!("{}", "-".repeat(110));

        let mut ok = 0usize;
        let mut mismatch = 0usize;

        for sec in &securities {
            let (pdf_bq, pdf_sq) = *out.pdf_totals.get(sec).unwrap();
            let parsed_buy  = trade_buy .get(sec).copied().unwrap_or(0.0);
            let parsed_sell = trade_sell.get(sec).copied().unwrap_or(0.0);
            let buy_ok  = (pdf_bq - parsed_buy).abs()  < 0.01;
            let sell_ok = (pdf_sq - parsed_sell).abs() < 0.01;
            let status  = if buy_ok && sell_ok { ok += 1; "OK" } else { mismatch += 1; "MISMATCH" };
            println!("{:<40}  {:>10.0}  {:>10.0}  {:>11.0}  {:>11.0}  {}",
                &sec[..sec.len().min(40)],
                pdf_bq, parsed_buy, pdf_sq, parsed_sell, status);
        }

        println!("\nResult: {} OK, {} MISMATCH out of {} securities", ok, mismatch, securities.len());
        if !out.skipped_details.is_empty() {
            println!("\nSKIPPED ({}):", out.skipped_details.len());
            for s in &out.skipped_details {
                println!("  {:?}  date={:?}  reason={}", s.security_name, s.raw_date, s.reason);
            }
        }
        assert_eq!(mismatch, 0, "{mismatch} securities have quantity mismatches");
    }

    /// Dumps ALL raw rows seen in the border grid for every data page.
    #[test]
    fn test_raw_rows_win() {
        if !std::path::Path::new(WIN_PDF).exists() {
            println!("SKIP: {WIN_PDF} not found");
            return;
        }
        let doc = lopdf::Document::load(WIN_PDF).expect("load PDF");
        let all_page_spans = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], X_GAP, CHAR_Y_TOL)
            .expect("extract spans");

        let mut page_nums: Vec<u32> = doc.get_pages().keys().copied().collect();
        page_nums.sort();

        for (page_idx, &page_num) in page_nums.iter().enumerate() {
            let page_spans = match all_page_spans.get(page_idx) {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            };
            let lines = pdf_utils::extract_page_lines(&doc, page_num);
            let (row_ys, col_xs) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);
            if col_xs.len() <= MIN_COLS { continue; }
            let padded = col_xs.iter().map(|&x| x + COL_PADDING).collect::<Vec<_>>();
            let cells  = pdf_utils::build_cell_map(page_spans, &row_ys, &padded, COL_SNAP);
            let n_rows = row_ys.len() + 1;

            println!("\n=== Page {} (PDF#{}) | {} cols | {} rows ===",
                page_idx + 1, page_num, col_xs.len(), n_rows);

            for row in (0..n_rows).rev() {
                let cols: Vec<String> = (0..col_xs.len())
                    .map(|c| pdf_utils::cell_text(&cells, row, c))
                    .collect();
                if cols.iter().all(|s| s.trim().is_empty()) { continue; }
                println!("  row{:>3} n_rows={}: {}",
                    row, n_rows,
                    cols.iter().map(|s| format!("[{:?}]", s.trim())).collect::<Vec<_>>().join(" "));
            }
        }
    }

    /// Generates an HTML grid visualisation of every page's parsed cells.
    /// Open C:\Users\SUMIT\AppData\Local\Temp\ce_global_grid.html in a browser.
    #[test]
    fn test_grid_viz_win() {
        if !std::path::Path::new(WIN_PDF).exists() {
            println!("SKIP: {WIN_PDF} not found");
            return;
        }

        let doc = lopdf::Document::load(WIN_PDF).expect("load PDF");
        let all_page_spans = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], X_GAP, CHAR_Y_TOL)
            .expect("extract spans");

        let col_headers = ["Security / BSE Code", "Date", "Buy Qty", "Buy Px",
                           "Sell Qty", "Sell Px", "Net Qty", "Net Px", "Net Val"];

        let mut page_nums: Vec<u32> = doc.get_pages().keys().copied().collect();
        page_nums.sort();

        let mut html = String::from(r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">
<title>CE Global Grid Visualisation</title>
<style>
  body { font: 12px monospace; background:#1a1a2e; color:#eee; margin:16px; }
  h2   { color:#a0cfff; margin:24px 0 6px; }
  .page-info { color:#888; font-size:11px; margin-bottom:8px; }
  table { border-collapse:collapse; margin-bottom:32px; max-width:100%; }
  th { background:#253060; color:#a0cfff; padding:4px 8px; border:1px solid #334; font-size:11px; }
  td { padding:3px 8px; border:1px solid #334; vertical-align:top; white-space:pre-wrap; max-width:220px; font-size:11px; }
  tr:nth-child(even) td { background:#1e2040; }
  tr:nth-child(odd)  td { background:#181830; }
  .row-idx { color:#556; font-size:10px; }
  .has-date { background:#1e3020 !important; }
  .section  { background:#2a2010 !important; }
  .empty    { color:#444; }
  .hline-info { color:#888; font-size:10px; margin:4px 0; }
</style>
</head><body>
<h1 style="color:#c0e0ff">CE Global — Border Grid Visualisation</h1>
<p style="color:#888">Constants: MIN_H_LEN=#MIN_H_LEN#  MIN_V_LEN=#MIN_V_LEN#  CLUSTER_GAP=#CLUSTER_GAP#  COL_PADDING=#COL_PADDING#  CHAR_Y_TOL=#CHAR_Y_TOL#</p>
"#);
        html = html
            .replace("#MIN_H_LEN#",   &MIN_H_LEN.to_string())
            .replace("#MIN_V_LEN#",   &MIN_V_LEN.to_string())
            .replace("#CLUSTER_GAP#", &CLUSTER_GAP.to_string())
            .replace("#COL_PADDING#", &COL_PADDING.to_string())
            .replace("#CHAR_Y_TOL#",  &CHAR_Y_TOL.to_string());

        for (page_idx, &page_num) in page_nums.iter().enumerate() {
            let page_spans = match all_page_spans.get(page_idx) {
                Some(s) if !s.is_empty() => s,
                _ => continue,
            };

            let lines = pdf_utils::extract_page_lines(&doc, page_num);
            let (row_ys, col_xs) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);

            let n_cols = col_xs.len();
            if n_cols == 0 { continue; }

            let padded_col_xs: Vec<f32> = col_xs.iter().map(|&x| x + COL_PADDING).collect();
            let cells  = pdf_utils::build_cell_map(page_spans, &row_ys, &padded_col_xs, COL_SNAP);
            let n_rows = row_ys.len() + 1;

            // H-line Y positions for context
            let h_ys_str: Vec<String> = row_ys.iter().map(|y| format!("{y:.1}")).collect();

            html.push_str(&format!(
                "<h2>Page {} (PDF page {})</h2>\n<div class=\"page-info\">cols={} rows={}  col_xs={:?}  h_lines=[{}]</div>\n",
                page_idx + 1, page_num, n_cols, n_rows,
                col_xs.iter().map(|x| format!("{x:.1}")).collect::<Vec<_>>(),
                h_ys_str.join(", ")
            ));

            html.push_str("<table><tr><th class=\"row-idx\">#</th>");
            let display_cols = n_cols.min(col_headers.len());
            for ci in 0..display_cols {
                html.push_str(&format!("<th>{}</th>", col_headers[ci]));
            }
            for ci in display_cols..n_cols {
                html.push_str(&format!("<th>Col {ci}</th>"));
            }
            html.push_str("</tr>\n");

            for row in 0..n_rows {
                let date_raw = pdf_utils::cell_text(&cells, row, COL_DATE);
                let sec_raw  = pdf_utils::cell_text(&cells, row, COL_SECURITY);

                // Determine row class for highlighting
                let row_class = if !date_raw.is_empty() && parse_equity_date(date_raw.trim()).is_some() {
                    "has-date"
                } else if !sec_raw.is_empty() && date_raw.is_empty() && section_re().is_match(sec_raw.trim()) {
                    "section"
                } else {
                    ""
                };

                // Skip completely empty rows in the visualisation
                let any_content = (0..n_cols).any(|c| !pdf_utils::cell_text(&cells, row, c).is_empty());
                if !any_content { continue; }

                html.push_str(&format!("<tr><td class=\"row-idx\">{row}</td>"));
                for col in 0..n_cols {
                    let txt = pdf_utils::cell_text(&cells, row, col);
                    let escaped = txt.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                    let cell_style = if !escaped.is_empty() { row_class } else { "empty" };
                    let display = if escaped.is_empty() { "·".to_string() } else { escaped };
                    html.push_str(&format!("<td class=\"{cell_style}\">{display}</td>"));
                }
                html.push_str("</tr>\n");
            }
            html.push_str("</table>\n");
        }

        html.push_str("</body></html>\n");

        let out_path = r"C:\Users\SUMIT\AppData\Local\Temp\ce_global_grid.html";
        std::fs::write(out_path, &html).expect("write HTML");
        println!("HTML written to {out_path}");
    }
}
