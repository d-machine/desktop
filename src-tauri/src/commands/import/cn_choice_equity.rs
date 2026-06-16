//! Parser for Choice Equity Broking — Contract Note PDF.
//!
//! Approach: extract stroked border lines per page → group V-lines into Y bands (one per
//! table) → derive band-specific column boundaries → assign text spans to cells using the
//! band's territory (column-header section + data rows below, bounded by an H-line gap).
//!
//! Handles equity and derivative segments in a single pass per page:
//!   Equity     — ~14-col table; data rows identified by ISIN in the first 6 columns.
//!   Derivative — ~9-col table; data rows identified by FUTSTK/FUTIDX/OPTSTK/OPTIDX prefix.
//!
//! Column offsets relative to the anchor column `k` (found by row scan):
//!   Equity:     k=ISIN  k+1=Name  k+2=BuyQty  k+3=BuyWAP  k+7=SellQty  k+8=SellWAP
//!   Derivative: k=Desc  k+1=Action  k+2=Qty  k+3=WAPfx(empty)  k+4=WAPRs  k+7=Closing

use crate::{commands::import::{common, pdf_utils, flag_oversells}, db};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ─── Extraction constants ─────────────────────────────────────────────────────

const X_GAP:         f32   = 6.0;   // max gap between characters within one span
const CHAR_Y_TOL:    f32   = 3.5;   // Y-baseline tolerance for span/row grouping
const MIN_H_LEN:     f32   = 20.0;  // min H-line length to count as a row boundary
// Settlement-header V-lines ≈22 pt are below this threshold and ignored.
// Equity column-header V-lines ≈83 pt and derivative ≈118 pt are above it.
const MIN_V_LEN:     f32   = 40.0;  // min V-line length to count as a column boundary
const CLUSTER_GAP:   f32   = 3.0;   // tolerance for collapsing near-duplicate line positions
const COL_SNAP:      f32   = 2.0;   // snap margin when assigning spans to columns
const COL_PADDING:   f32   = 4.0;   // shift col left-edges right before cell assignment
const MIN_DATA_COLS: usize = 6;     // minimum column count for a page to contain a data table
// Typical row height is 10–15 pt; a gap larger than this separates table from footer.
const TABLE_ROW_GAP: f32   = 25.0;  // H-line gap marking the transition to footer/next section

// ─── Regexes ─────────────────────────────────────────────────────────────────

fn isin_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^IN[A-Z0-9]{10}$").unwrap())
}

fn deriv_prefix_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)^(FUTSTK|FUTIDX|OPTSTK|OPTIDX)\b").unwrap())
}

fn deriv_desc_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| {
        // FUTSTK SYMBOL DDMonYYYY-EXCHANGE
        // OPTSTK SYMBOL DDMonYYYY STRIKE CE|PE-EXCHANGE  (strike comes before option type)
        Regex::new(
            r"(?i)^(FUTSTK|FUTIDX|OPTSTK|OPTIDX)\s+(\S+)\s+(\d{2}[A-Za-z]{3}\d{4})(?:\s+([\d.]+)\s+([A-Z]{2,3}))?-(\w+)"
        ).unwrap()
    })
}

fn trade_date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)Trade\s*Date\s*[:\s]+(\d{2}/\d{2}/\d{4})").unwrap())
}

fn cn_no_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)CONTRACT\s+NOTE\s+NO\s*[:\s]+(\d+)").unwrap())
}

fn dr_amount_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"([\d,]+\.\d+)\s*DR").unwrap())
}


// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedCharge {
    pub charge_type:  String,
    pub amount_paise: i64,
}

/// One row from the "Obligation Details" table — description + total only.
/// Positive = DR (pay-in / you owe), negative = CR (pay-out / you receive).
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedObligation {
    pub description: String,
    pub total_paise: i64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedEquityRow {
    pub isin:          String,
    pub security_name: String,
    pub bse_code:      String,
    pub txn_type:      String,  // BUY | SELL
    pub quantity:      f64,
    pub price:         f64,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ParsedDerivRow {
    pub contract_desc: String,
    pub kind:          String,          // FUTSTK | FUTIDX | OPTSTK | OPTIDX
    pub underlying:    String,
    pub expiry_date:   String,          // ISO YYYY-MM-DD
    pub exchange:      String,
    pub txn_type:      String,          // BUY | SELL | BF | CF
    pub quantity:      f64,
    pub price:         f64,
    pub strike_paise:  Option<i64>,
    pub option_type:   Option<String>,
}

#[derive(Debug, Serialize)]
pub struct CnChoiceEquityParseResult {
    pub trade_date:    String,
    pub cn_number:     String,
    pub client_code:   Option<String>,
    pub client_name:   Option<String>,
    pub equity_rows:   Vec<ParsedEquityRow>,
    pub deriv_rows:    Vec<ParsedDerivRow>,
    pub charges:       Vec<ParsedCharge>,
    pub obligations:   Vec<ParsedObligation>,
    pub pages_scanned: usize,
    pub skipped_rows:  usize,
}

#[derive(Debug, Serialize)]
pub struct CnImportResult {
    pub imported:                 usize,
    pub skipped:                  usize,
    pub auto_created_instruments: usize,
}

// ─── Helper functions ─────────────────────────────────────────────────────────

fn month_num(abbr: &str) -> Option<u32> {
    match abbr.to_lowercase().as_str() {
        "jan" => Some(1),  "feb" => Some(2),  "mar" => Some(3),
        "apr" => Some(4),  "may" => Some(5),  "jun" => Some(6),
        "jul" => Some(7),  "aug" => Some(8),  "sep" => Some(9),
        "oct" => Some(10), "nov" => Some(11), "dec" => Some(12),
        _ => None,
    }
}

// "30Mar2026" → "2026-03-30"
fn parse_expiry(s: &str) -> Option<String> {
    if s.len() < 9 { return None; }
    let day:   u32 = s[..2].parse().ok()?;
    let month        = month_num(&s[2..5])?;
    let year:  u32 = s[5..].parse().ok()?;
    Some(format!("{year}-{month:02}-{day:02}"))
}

// "27/04/2026" → "2026-04-27"
fn parse_trade_date_str(s: &str) -> Option<String> {
    let p: Vec<&str> = s.splitn(3, '/').collect();
    if p.len() != 3 { return None; }
    let d: u32 = p[0].parse().ok()?;
    let m: u32 = p[1].parse().ok()?;
    let y: u32 = p[2].parse().ok()?;
    if d < 1 || d > 31 || m < 1 || m > 12 { return None; }
    Some(format!("{y}-{m:02}-{d:02}"))
}

fn parse_num(s: &str) -> f64 {
    let s = s.trim();
    if s.is_empty() || s == "-" { return 0.0; }
    s.replace(',', "").parse().unwrap_or(0.0)
}

// "1,234.56 DR" → Some(123456)  "1,234.56 CR" → Some(-123456)
fn parse_obligation_amount(s: &str) -> Option<i64> {
    let s = s.trim();
    if s.is_empty() || s == "-" { return None; }
    let (num, sign) = if s.ends_with("DR") {
        (s[..s.len() - 2].trim(), 1i64)
    } else if s.ends_with("CR") {
        (s[..s.len() - 2].trim(), -1i64)
    } else {
        (s, 1i64)
    };
    let v: f64 = num.replace(',', "").parse().ok()?;
    if v == 0.0 { return None; }
    Some((v * 100.0).round() as i64 * sign)
}

// "RELAXO FOOTWEARS LTD./530517" → ("RELAXO FOOTWEARS LTD.", "530517")
fn split_name_bse(s: &str) -> (String, String) {
    if let Some(idx) = s.rfind('/') {
        let name = s[..idx].trim().to_string();
        let code = s[idx + 1..].trim().to_string();
        (name, code)
    } else {
        (s.trim().to_string(), String::new())
    }
}

fn action_to_txn_type(action: &str) -> &'static str {
    match action.trim().to_uppercase().as_str() {
        "B"  => "BUY",
        "S"  => "SELL",
        "BF" => "BF",
        "CF" => "CF",
        _    => "BUY",
    }
}

// Returns (kind, symbol, expiry_iso, exchange, strike_paise, option_type)
fn parse_contract_desc(
    desc: &str,
) -> Option<(String, String, String, String, Option<i64>, Option<String>)> {
    let caps = deriv_desc_re().captures(desc.trim())?;
    let kind     = caps[1].to_uppercase();
    let symbol   = caps[2].to_uppercase();
    let expiry   = parse_expiry(&caps[3])?;
    let exchange = caps.get(6)
        .map(|m| m.as_str().to_uppercase())
        .unwrap_or_else(|| "NSE".to_string());

    let (strike_paise, option_type) = if kind.starts_with("OPT") {
        // group 4 = strike (numeric), group 5 = option type (CE/PE)
        let sp = caps.get(4)
            .and_then(|m| m.as_str().parse::<f64>().ok())
            .map(|p| (p * 100.0).round() as i64);
        let ot = caps.get(5).map(|m| m.as_str().to_uppercase());
        (sp, ot)
    } else {
        (None, None)
    };

    Some((kind, symbol, expiry, exchange, strike_paise, option_type))
}

// ─── Header extraction ────────────────────────────────────────────────────────

fn extract_header(
    all_pages: &[Vec<pdf_utils::TextSpan>],
) -> (String, String, Option<String>, Option<String>) {
    let mut trade_date  = String::new();
    let mut cn_number   = String::new();
    let mut client_code = None::<String>;
    let mut client_name = None::<String>;

    if let Some(page0) = all_pages.first() {
        let full: String = page0.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");

        if let Some(caps) = trade_date_re().captures(&full) {
            trade_date = parse_trade_date_str(&caps[1]).unwrap_or_else(|| caps[1].to_string());
        }
        if let Some(caps) = cn_no_re().captures(&full) {
            cn_number = caps[1].to_string();
        }

        // Scan rows for client code and name
        let rows = pdf_utils::page_spans_to_rows(page0.clone(), 5.0);
        for row in &rows {
            for (i, span) in row.iter().enumerate() {
                let lc = span.text.trim().to_lowercase();
                if (lc.contains("ucc") && lc.contains("client code")) && client_code.is_none() {
                    let val = row.get(i + 1)
                        .map(|s| s.text.trim_start_matches(':').trim().to_string())
                        .filter(|s| !s.is_empty());
                    if val.is_some() { client_code = val; }
                }
                if lc.contains("name of the client") && client_name.is_none() {
                    let val = row.get(i + 1)
                        .map(|s| s.text.trim_start_matches(':').trim().to_string())
                        .filter(|s| !s.is_empty());
                    if val.is_some() { client_name = val; }
                }
            }
        }
    }

    (trade_date, cn_number, client_code, client_name)
}

// ─── Charge extraction ────────────────────────────────────────────────────────

fn extract_dr_amount(text: &str) -> Option<f64> {
    // Take the LAST DR match — in multi-segment CNs the obligation table has
    // per-segment columns before the Total column; the last DR value is the total.
    dr_amount_re().captures_iter(text)
        .filter_map(|c| c[1].replace(',', "").parse::<f64>().ok())
        .filter(|&v| v > 0.0)
        .last()
}

fn extract_charges(all_pages: &[Vec<pdf_utils::TextSpan>]) -> Vec<ParsedCharge> {
    let mut accum: std::collections::HashMap<String, i64> = Default::default();

    for page in all_pages {
        let rows = pdf_utils::page_spans_to_rows(page.clone(), 3.0);
        for row in &rows {
            let row_text: String = row.iter()
                .map(|s| s.text.trim())
                .filter(|s| !s.is_empty())
                .collect::<Vec<_>>()
                .join(" ");
            if !row_text.contains("DR") { continue; }
            let lc = row_text.to_lowercase();

            let key = if lc.contains("brokerage") && lc.contains("supply") {
                Some("BROKERAGE")
            } else if lc.contains("toc nse exchange") || (lc.contains("exchange") && lc.contains("supply")) {
                Some("TRANSACTION_CHARGES")
            } else if lc.contains("clearing") && lc.contains("supply") {
                Some("CLEARING_CHARGES")
            } else if lc.contains("sebi") && lc.contains("supply") {
                Some("SEBI_TURNOVER_FEES")
            } else if lc.contains("cgst") || lc.contains("sgst") {
                Some("GST")
            } else if lc.contains("stamp duty") {
                Some("STAMP_DUTY")
            } else if lc.contains("securities transaction") {
                Some("STT")
            } else {
                None
            };

            if let Some(k) = key {
                if let Some(amount) = extract_dr_amount(&row_text) {
                    *accum.entry(k.to_string()).or_insert(0) += (amount * 100.0).round() as i64;
                }
            }
        }
    }

    accum.into_iter()
        .filter(|(_, p)| *p > 0)
        .map(|(ct, p)| ParsedCharge { charge_type: ct, amount_paise: p })
        .collect()
}

fn extract_obligations(all_pages: &[Vec<pdf_utils::TextSpan>]) -> Vec<ParsedObligation> {
    for page in all_pages {
        let rows = pdf_utils::page_spans_to_rows(page.clone(), 3.0);

        // Find the header row: it contains "ICCLCM" (the first segment column header).
        let Some(hdr_idx) = rows.iter().position(|row| {
            row.iter().any(|s| s.text.trim().to_uppercase().contains("ICCLCM"))
        }) else { continue };

        let hdr_row = &rows[hdr_idx];
        let col_xs: Vec<f32> = hdr_row.iter().map(|s| s.x).collect();

        // Locate the "TOTAL" column by header text.
        let ci_total: Option<usize> = hdr_row.iter().position(|s| {
            s.text.trim().to_uppercase().contains("TOTAL")
        });

        let mut obligations = Vec::new();
        for row in &rows[hdr_idx + 1..] {
            if row.iter().all(|s| s.text.trim().is_empty()) { break; }
            let cells = pdf_utils::spans_to_cells(row, &col_xs);
            let description = cells.first().map(|s| s.trim().to_string()).unwrap_or_default();
            if description.is_empty() { continue; }
            let total_paise = ci_total
                .and_then(|i| cells.get(i))
                .and_then(|s| parse_obligation_amount(s))
                .unwrap_or(0);
            obligations.push(ParsedObligation { description, total_paise });
        }

        if !obligations.is_empty() { return obligations; }
    }
    Vec::new()
}

// ─── Core parser ──────────────────────────────────────────────────────────────

struct ParseOutput {
    equity_rows:   Vec<ParsedEquityRow>,
    deriv_rows:    Vec<ParsedDerivRow>,
    skipped:       usize,
    pages_scanned: usize,
}

fn parse_pdf(doc: &lopdf::Document, all_page_spans: &[Vec<pdf_utils::TextSpan>]) -> ParseOutput {
    let mut equity_rows:  Vec<ParsedEquityRow> = Vec::new();
    let mut deriv_rows:   Vec<ParsedDerivRow>  = Vec::new();
    let mut skipped       = 0usize;
    let mut pages_scanned = 0usize;

    let mut page_nums: Vec<u32> = doc.get_pages().keys().copied().collect();
    page_nums.sort();

    for (page_idx, &page_num) in page_nums.iter().enumerate() {
        let page_spans = match all_page_spans.get(page_idx) {
            Some(s) if !s.is_empty() => s,
            _ => continue,
        };

        pages_scanned += 1;
        let lines = pdf_utils::extract_page_lines(doc, page_num);
        let bands = pdf_utils::grid_from_lines_banded(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);

        // Use all page H-lines for row boundaries — the band's V-lines only span the
        // column-header section, so per-row separators would be missed if we filtered
        // H-lines by band Y extent.  Rows outside a band's territory are empty → skipped.
        let (page_row_ys, _) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, f32::MAX, CLUSTER_GAP);

        // Territory [lo, hi] for each band:
        //   hi = band.y_max  (top of column headers; never extend into the page header)
        //   lo = walk H-lines downward from band.y_min, stop at the first gap > TABLE_ROW_GAP
        //        (that gap marks the table bottom / footer boundary).
        //        On multi-band pages the walk is also fenced above the next-lower band.
        let territory: Vec<(f32, f32)> = {
            let n = bands.len();
            (0..n).map(|i| {
                let hi = bands[i].y_max;
                let lower_fence = if i == 0 { f32::NEG_INFINITY } else { bands[i - 1].y_max };
                let mut below: Vec<f32> = page_row_ys.iter()
                    .filter(|&&y| y < bands[i].y_min - CLUSTER_GAP && y > lower_fence)
                    .copied()
                    .collect();
                below.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
                let mut lo = bands[i].y_min;
                let mut prev = bands[i].y_min;
                for y in below {
                    if prev - y > TABLE_ROW_GAP { break; }
                    lo = y;
                    prev = y;
                }
                (lo, hi)
            }).collect()
        };

        for (band, &(terr_lo, terr_hi)) in bands.iter().zip(territory.iter()) {
            let col_xs = &band.col_xs;
            if col_xs.len() < MIN_DATA_COLS { continue; }

            let band_spans: Vec<_> = page_spans.iter()
                .filter(|s| s.y >= terr_lo - CHAR_Y_TOL && s.y <= terr_hi + CHAR_Y_TOL)
                .cloned()
                .collect();

            let padded: Vec<f32> = col_xs.iter().map(|&x| x + COL_PADDING).collect();
            let cells  = pdf_utils::build_cell_map(&band_spans, &page_row_ys, &padded, COL_SNAP);
            let n_rows = page_row_ys.len() + 1;
            let n_cols = col_xs.len();

            for row in (0..n_rows).rev() {
                let mut data_col: Option<usize> = None;
                let mut is_equity = false;

                for c in 0..n_cols.min(6) {
                    let txt = pdf_utils::cell_text(&cells, row, c);
                    let t   = txt.trim();
                    if isin_re().is_match(t) {
                        data_col = Some(c);
                        is_equity = true;
                        break;
                    }
                    if deriv_prefix_re().is_match(t) {
                        data_col = Some(c);
                        is_equity = false;
                        break;
                    }
                }

                let k = match data_col {
                    Some(c) => c,
                    None    => continue,
                };

                if is_equity {
                    let isin     = pdf_utils::cell_text(&cells, row, k).trim().to_string();
                    let name_raw = pdf_utils::cell_text(&cells, row, k + 1);
                    let buy_qty  = parse_num(&pdf_utils::cell_text(&cells, row, k + 2));
                    let buy_wap  = parse_num(&pdf_utils::cell_text(&cells, row, k + 3));
                    let sell_qty = parse_num(&pdf_utils::cell_text(&cells, row, k + 7));
                    let sell_wap = parse_num(&pdf_utils::cell_text(&cells, row, k + 8));

                    let (security_name, bse_code) = split_name_bse(&name_raw);
                    let segment = if buy_qty > 0.0 && sell_qty > 0.0 { "INTRADAY" } else { "DELIVERY" };

                    if buy_qty > 0.0 {
                        equity_rows.push(ParsedEquityRow {
                            isin:          isin.clone(),
                            security_name: security_name.clone(),
                            bse_code:      bse_code.clone(),
                            txn_type:      format!("BUY|{segment}"),
                            quantity:      buy_qty,
                            price:         buy_wap,
                        });
                    }
                    if sell_qty > 0.0 {
                        equity_rows.push(ParsedEquityRow {
                            isin:          isin,
                            security_name: security_name,
                            bse_code:      bse_code,
                            txn_type:      format!("SELL|{segment}"),
                            quantity:      sell_qty,
                            price:         sell_wap,
                        });
                    }
                    if buy_qty == 0.0 && sell_qty == 0.0 {
                        skipped += 1;
                    }
                } else {
                    // Derivative row
                    let desc   = pdf_utils::cell_text(&cells, row, k).trim().to_string();
                    let action = pdf_utils::cell_text(&cells, row, k + 1);
                    let qty_s  = pdf_utils::cell_text(&cells, row, k + 2);
                    // k+3 = WAP foreign currency (empty for INR contracts)
                    let wap_s  = pdf_utils::cell_text(&cells, row, k + 4);

                    let txn_type = action_to_txn_type(action.trim());
                    let quantity = parse_num(&qty_s).abs();
                    let price    = parse_num(&wap_s);

                    if quantity == 0.0 {
                        skipped += 1;
                        continue;
                    }

                    match parse_contract_desc(&desc) {
                        Some((kind, underlying, expiry_date, exchange, strike_paise, option_type)) => {
                            deriv_rows.push(ParsedDerivRow {
                                contract_desc: desc,
                                kind,
                                underlying,
                                expiry_date,
                                exchange,
                                txn_type: txn_type.to_string(),
                                quantity,
                                price,
                                strike_paise,
                                option_type,
                            });
                        }
                        None => { skipped += 1; }
                    }
                }
            }
        }
    }

    ParseOutput { equity_rows, deriv_rows, skipped, pages_scanned }
}

// ─── Tauri commands ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn parse_cn_choice_equity_pdf(
    file_path: String,
    password:  Option<String>,
) -> Result<CnChoiceEquityParseResult, String> {
    let doc = pdf_utils::load_pdf(&file_path, password.as_deref())?;
    let all_page_spans = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], X_GAP, CHAR_Y_TOL)?;

    let (trade_date, cn_number, client_code, client_name) = extract_header(&all_page_spans);
    let charges     = extract_charges(&all_page_spans);
    let obligations = extract_obligations(&all_page_spans);
    let out         = parse_pdf(&doc, &all_page_spans);

    Ok(CnChoiceEquityParseResult {
        trade_date,
        cn_number,
        client_code,
        client_name,
        equity_rows:   out.equity_rows,
        deriv_rows:    out.deriv_rows,
        charges,
        obligations,
        pages_scanned: out.pages_scanned,
        skipped_rows:  out.skipped,
    })
}

#[tauri::command]
pub fn import_cn_choice_equity_trades(
    app:         tauri::AppHandle,
    account_id:  i64,
    trade_date:  String,
    cn_number:   String,
    equity_rows: Vec<ParsedEquityRow>,
    deriv_rows:  Vec<ParsedDerivRow>,
    charges:     Vec<ParsedCharge>,
    file_paths:  Option<Vec<String>>,
) -> Result<CnImportResult, String> {
    let conn         = db::acquire()?;
    let mut imported     = 0usize;
    let mut skipped      = 0usize;
    let mut auto_created = 0usize;

    let ref_no = if cn_number.is_empty() || cn_number == "0" {
        file_paths.as_ref()
            .and_then(|ps| ps.first())
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("CN-CE-{}", chrono::Utc::now().timestamp()))
    } else {
        format!("CN-CE-{cn_number}")
    };

    let existing_batch_id: Option<i64> = conn.query_row(
        "SELECT batch_id FROM import_batches
         WHERE account_id=?1 AND source_type='CN_CHOICE_EQUITY' AND ref_no=?2",
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
             VALUES (?1, 'CN_CHOICE_EQUITY', ?2, 'Choice Equity Broking', ?3)",
            rusqlite::params![account_id, ref_no, trade_date],
        ).map_err(|e| e.to_string())?;
        conn.last_insert_rowid()
    };

    // Import equity rows
    // txn_type is encoded as "BUY|DELIVERY", "SELL|INTRADAY" etc.
    for row in &equity_rows {
        let (txn_type, segment) = row.txn_type.split_once('|')
            .unwrap_or((&row.txn_type, "DELIVERY"));

        let (instrument_id, pending_instrument_id) = match common::resolve_equity(
            &conn,
            &row.security_name,
            Some(&row.isin),
            if row.bse_code.is_empty() { None } else { Some(&row.bse_code) },
            None,
            None,
            &mut auto_created,
        ) {
            Some(pair) => pair,
            None => { skipped += 1; continue; }
        };

        let price_paise = (row.price * 100.0).round() as i64;
        let qty_milli   = (row.quantity * 1000.0).round() as i64;
        let broker_ref  = format!(
            "CN-CE-EQ-{}-{}-{}-{}",
            row.isin, trade_date, txn_type, qty_milli,
        );
        let gross_paise = (row.quantity * row.price * 100.0).round() as i64;
        let total_value = if txn_type == "BUY" { -gross_paise } else { gross_paise };

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
                txn_type, segment, trade_date,
                row.quantity, price_paise, total_value, broker_ref, batch_id,
            ],
        ).map_err(|e| e.to_string())?;

        if rows > 0 { imported += 1; } else { skipped += 1; }
    }

    // Import derivative rows
    for row in &deriv_rows {
        let instrument_type = if row.kind.starts_with("FUT") { "FUTURES" } else { "OPTIONS" };
        let pending_name    = format!("{} {} {}", row.kind, row.underlying, row.expiry_date);

        let (instrument_id, pending_instrument_id) = match common::resolve_derivative(
            &conn,
            &pending_name,
            &row.underlying,
            &row.expiry_date,
            &row.kind,
            instrument_type,
            &row.exchange,
            row.strike_paise,
            row.option_type.as_deref(),
            &mut auto_created,
        ) {
            Some(pair) => pair,
            None => { skipped += 1; continue; }
        };

        let price_paise = (row.price * 100.0).round() as i64;
        let qty_milli   = (row.quantity * 1000.0).round() as i64;
        let broker_ref  = format!(
            "CN-CE-DRV-{}-{}-{}-{}-{}",
            trade_date, row.underlying, row.expiry_date, row.txn_type, qty_milli,
        );
        let gross_paise = (row.quantity * row.price * 100.0).round() as i64;
        let total_value = match row.txn_type.as_str() {
            "BUY" | "BF" => -gross_paise,
            _             =>  gross_paise,
        };

        let rows = conn.execute(
            "INSERT INTO transactions
                (account_id, instrument_id, pending_instrument_id, txn_type, trade_segment,
                 trade_date, quantity, price_paise, brokerage_paise, stt_paise,
                 other_charges_paise, total_value_paise, notes, broker_ref, batch_id)
             VALUES (?1,?2,?3,?4,'FNO',?5,?6,?7,0,0,0,?8,NULL,?9,?10)
             ON CONFLICT(account_id, broker_ref) WHERE broker_ref IS NOT NULL DO UPDATE SET
                 instrument_id         = excluded.instrument_id,
                 pending_instrument_id = excluded.pending_instrument_id,
                 batch_id              = excluded.batch_id
             WHERE transactions.instrument_id         IS NOT excluded.instrument_id
                OR transactions.pending_instrument_id IS NOT excluded.pending_instrument_id",
            rusqlite::params![
                account_id, instrument_id, pending_instrument_id,
                row.txn_type, trade_date,
                row.quantity, price_paise, total_value, broker_ref, batch_id,
            ],
        ).map_err(|e| e.to_string())?;

        if rows > 0 { imported += 1; } else { skipped += 1; }
    }

    // Insert charges (idempotent: only for new batches)
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

    Ok(CnImportResult { imported, skipped, auto_created_instruments: auto_created })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const EQ_PDF: &str =
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\CN_D03695_Grp1_255322.PDF";

    const DERIV_PDF: &str =
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\02.03.2026.-CHOICE EQUITY.PDF";

    const NEW_CN_PDF: &str =
        r"C:\Users\SUMIT\Downloads\CN_D03695_Grp1_372682.PDF";

    fn skip_if_missing(path: &str) -> bool {
        if !std::path::Path::new(path).exists() {
            println!("SKIP: {path} not found");
            true
        } else {
            false
        }
    }

    #[test]
    fn test_parse_equity_cn() {
        if skip_if_missing(EQ_PDF) { return; }
        let result = parse_cn_choice_equity_pdf(EQ_PDF.to_string(), None).expect("parse failed");
        println!("Trade date: {}  CN#: {}  client: {:?}",
            result.trade_date, result.cn_number, result.client_code);
        println!("Equity rows: {}  Deriv rows: {}  Skipped: {}",
            result.equity_rows.len(), result.deriv_rows.len(), result.skipped_rows);
        for r in &result.equity_rows {
            println!("  EQ {} {} {:6} qty={:>8.2} @ {:>10.4}  {} ({})",
                r.isin, r.txn_type, r.bse_code, r.quantity, r.price, r.security_name, r.bse_code);
        }
        for c in &result.charges {
            println!("  Charge {} = {:.2}", c.charge_type, c.amount_paise as f64 / 100.0);
        }
        assert!(!result.equity_rows.is_empty(), "expected equity rows");
    }

    #[test]
    fn test_parse_deriv_cn() {
        if skip_if_missing(DERIV_PDF) { return; }
        let result = parse_cn_choice_equity_pdf(DERIV_PDF.to_string(), None).expect("parse failed");
        println!("Trade date: {}  CN#: {}  client: {:?}",
            result.trade_date, result.cn_number, result.client_code);
        println!("Equity rows: {}  Deriv rows: {}  Skipped: {}",
            result.equity_rows.len(), result.deriv_rows.len(), result.skipped_rows);
        for r in result.deriv_rows.iter().take(10) {
            println!("  DRV {:6} {:8} {:10} expiry={} qty={:>10.0} @ {:>10.4}",
                r.kind, r.txn_type, r.underlying, r.expiry_date, r.quantity, r.price);
        }
        assert!(!result.deriv_rows.is_empty(), "expected derivative rows");
    }

    #[test]
    fn test_parse_new_cn() {
        if skip_if_missing(NEW_CN_PDF) { return; }
        let result = parse_cn_choice_equity_pdf(NEW_CN_PDF.to_string(), None).expect("parse failed");
        println!("Trade date: {}  CN#: {}  client: {:?}  client_name: {:?}",
            result.trade_date, result.cn_number, result.client_code, result.client_name);
        println!("Equity rows: {}  Deriv rows: {}  Skipped: {}  Pages: {}",
            result.equity_rows.len(), result.deriv_rows.len(), result.skipped_rows, result.pages_scanned);
        println!("\n--- EQUITY ---");
        for r in &result.equity_rows {
            let (txn, seg) = r.txn_type.split_once('|').unwrap_or((&r.txn_type, ""));
            println!("  {:4} {:8} ISIN={} BSE={:7} qty={:>8.0} @ {:>10.4}  {}",
                txn, seg, r.isin, r.bse_code, r.quantity, r.price, r.security_name);
        }
        println!("\n--- DERIVATIVES ---");
        for r in &result.deriv_rows {
            println!("  {:6} {:4} {:10} expiry={} strike={:?} opt={:?}  qty={:>8.0} @ {:>10.4}",
                r.kind, r.txn_type, r.underlying, r.expiry_date,
                r.strike_paise, r.option_type, r.quantity, r.price);
        }
        println!("\n--- CHARGES ---");
        for c in &result.charges {
            println!("  {} = {:.2}", c.charge_type, c.amount_paise as f64 / 100.0);
        }
        println!("\n--- OBLIGATIONS ---");
        for o in &result.obligations {
            println!("  {} = {:.2}", o.description, o.total_paise as f64 / 100.0);
        }
    }

    /// Full banded-cell visualisation for all three CNs.
    ///
    /// For each page, each detected band is shown with:
    ///   - Shaded territory region (column headers + data rows, bounded by H-line gap)
    ///   - Band-specific column boundaries (in the band colour)
    ///   - Raw PDF lines + text spans
    ///   - Cell contents table below the SVG, with equity / derivative rows highlighted
    ///
    /// Saved to %TEMP%\cn_banded_data.html
    #[test]
    fn test_html_banded_data() {
        let band_colours: &[(&str, &str, &str)] = &[
            ("rgba(255,180,0,0.10)",  "rgb(190,120,0)",  "rgb(150,90,0)"),   // amber
            ("rgba(80,60,220,0.08)", "rgb(70,40,190)",  "rgb(50,20,160)"),  // indigo
            ("rgba(0,160,110,0.08)", "rgb(0,130,90)",   "rgb(0,100,70)"),   // teal
            ("rgba(200,0,90,0.08)",  "rgb(180,0,70)",   "rgb(140,0,50)"),   // crimson
        ];

        let mut full_body = String::new();

        for (label, path) in [("EQUITY_CN", EQ_PDF), ("DERIV_CN", DERIV_PDF), ("NEW_CN", NEW_CN_PDF)] {
            if skip_if_missing(path) { continue; }

            let doc = lopdf::Document::load(path).expect("load PDF");
            let spans_all = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], X_GAP, CHAR_Y_TOL)
                .expect("extract spans");
            let mut page_nums: Vec<u32> = doc.get_pages().keys().copied().collect();
            page_nums.sort();

            for (pi, &pn) in page_nums.iter().enumerate().take(3) {
                let ps = match spans_all.get(pi) { Some(s) if !s.is_empty() => s, _ => continue };
                let lines = pdf_utils::extract_page_lines(&doc, pn);
                let bands = pdf_utils::grid_from_lines_banded(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);
                let (page_row_ys, _) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, f32::MAX, CLUSTER_GAP);

                // Same territory logic as parse_pdf
                let n_bands = bands.len();
                let territory: Vec<(f32, f32)> = (0..n_bands).map(|i| {
                    let hi = bands[i].y_max;
                    let lower_fence = if i == 0 { f32::NEG_INFINITY }
                                      else { bands[i - 1].y_max };
                    let mut below: Vec<f32> = page_row_ys.iter()
                        .filter(|&&y| y < bands[i].y_min - CLUSTER_GAP && y > lower_fence)
                        .copied()
                        .collect();
                    below.sort_by(|a, b| b.partial_cmp(a).unwrap_or(std::cmp::Ordering::Equal));
                    let mut lo = bands[i].y_min;
                    let mut prev = bands[i].y_min;
                    for y in below {
                        if prev - y > TABLE_ROW_GAP { break; }
                        lo = y;
                        prev = y;
                    }
                    (lo, hi)
                }).collect();

                let page_w = lines.iter().flat_map(|l| [l.x1, l.x2]).fold(595.0_f32, f32::max);
                let page_h = lines.iter().flat_map(|l| [l.y1, l.y2]).fold(842.0_f32, f32::max);
                let scale  = 1.2_f32;
                let svg_w  = (page_w * scale).ceil() as u32 + 20;
                let svg_h  = (page_h * scale).ceil() as u32 + 40;
                let fy = |y: f32| -> f32 { (page_h - y) * scale + 30.0 };
                let fx = |x: f32| -> f32 { x * scale };

                let mut svg = format!(
                    r##"<svg xmlns="http://www.w3.org/2000/svg" width="{svg_w}" height="{svg_h}" style="background:#fff;display:block">"##
                );

                // Territory fill rectangles
                for (bi, (&(terr_lo, terr_hi), band)) in territory.iter().zip(bands.iter()).enumerate() {
                    let (fill, stroke, label_clr) = band_colours[bi.min(band_colours.len() - 1)];
                    let top    = fy(terr_hi.min(page_h));
                    let bottom = fy(terr_lo.max(0.0));
                    let height = (bottom - top).max(0.0);
                    svg.push_str(&format!(
                        r#"<rect x="0" y="{top:.1}" width="{svg_w}" height="{height:.1}" fill="{fill}"/>"#
                    ));
                    // Band label
                    let lbl_y = top + height / 2.0 + 4.0;
                    svg.push_str(&format!(
                        "<text x=\"3\" y=\"{lbl_y:.0}\" fill=\"{label_clr}\" font-size=\"10\" font-family=\"sans-serif\" font-weight=\"bold\">band {bi} ({} cols)</text>",
                        band.col_xs.len(),
                    ));
                    // Top of column-header V-lines (band.y_max) — dashed line
                    let hdr_top = fy(band.y_max);
                    svg.push_str(&format!(
                        r#"<line x1="0" y1="{hdr_top:.1}" x2="{svg_w}" y2="{hdr_top:.1}" stroke="{stroke}" stroke-width="1.5" stroke-dasharray="5,3" opacity="0.6"/>"#
                    ));
                    // Bottom of column-header V-lines (band.y_min) — dashed line
                    let hdr_bot = fy(band.y_min);
                    svg.push_str(&format!(
                        r#"<line x1="0" y1="{hdr_bot:.1}" x2="{svg_w}" y2="{hdr_bot:.1}" stroke="{stroke}" stroke-width="1.5" stroke-dasharray="5,3" opacity="0.6"/>"#
                    ));
                    // Column boundaries (full territory height, band colour)
                    for (ci, &cx) in band.col_xs.iter().enumerate() {
                        let sx = fx(cx);
                        svg.push_str(&format!(
                            r#"<line x1="{sx:.1}" y1="{top:.1}" x2="{sx:.1}" y2="{bottom:.1}" stroke="{stroke}" stroke-width="1.0" opacity="0.55"/>"#
                        ));
                        svg.push_str(&format!(
                            "<text x=\"{:.1}\" y=\"{:.0}\" fill=\"{stroke}\" font-size=\"8\" font-family=\"monospace\" font-weight=\"bold\" text-anchor=\"middle\">{ci}</text>",
                            sx + 3.0, top - 2.0,
                        ));
                    }
                }

                // Raw PDF lines
                for l in &lines {
                    if l.length() < 3.0 { continue; }
                    let color = if l.is_vertical(1.0) { "#c0c0c0" } else { "#dedede" };
                    svg.push_str(&format!(
                        r#"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{color}" stroke-width="0.6"/>"#,
                        fx(l.x1), fy(l.y1), fx(l.x2), fy(l.y2)
                    ));
                }

                // Page-wide row boundaries (light blue)
                for &ry in &page_row_ys {
                    let sy = fy(ry);
                    svg.push_str(&format!(
                        r#"<line x1="0" y1="{sy:.1}" x2="{svg_w}" y2="{sy:.1}" stroke="rgba(0,80,200,0.18)" stroke-width="0.7"/>"#
                    ));
                }

                // Text spans
                for span in ps.iter() {
                    let x = fx(span.x); let y = fy(span.y) - 1.5;
                    let t = span.text.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;").replace('"',"&quot;");
                    svg.push_str(&format!(
                        "<text x=\"{x:.1}\" y=\"{y:.1}\" fill=\"rgb(20,20,20)\" font-size=\"7\" font-family=\"monospace\">{t}</text>"
                    ));
                }
                svg.push_str("</svg>");

                // Cell tables — one per qualifying band
                let mut tables_html = String::new();
                for (bi, (&(terr_lo, terr_hi), band)) in territory.iter().zip(bands.iter()).enumerate() {
                    if band.col_xs.len() < MIN_DATA_COLS { continue; }
                    let (_, stroke, label_clr) = band_colours[bi.min(band_colours.len() - 1)];

                    let band_spans: Vec<_> = ps.iter()
                        .filter(|s| s.y >= terr_lo - CHAR_Y_TOL && s.y <= terr_hi + CHAR_Y_TOL)
                        .cloned()
                        .collect();

                    let padded: Vec<f32> = band.col_xs.iter().map(|&x| x + COL_PADDING).collect();
                    let cells  = pdf_utils::build_cell_map(&band_spans, &page_row_ys, &padded, COL_SNAP);
                    let n_rows = page_row_ys.len() + 1;
                    let n_cols = band.col_xs.len();

                    let mut tbl = format!(
                        "<table style=\"border-collapse:collapse;font-size:10px;font-family:monospace;margin-top:4px\"><thead><tr><th style=\"border:1px solid #bbb;padding:2px 5px;background:#f0f0f0\">row</th>"
                    );
                    for ci in 0..n_cols {
                        tbl.push_str(&format!(
                            "<th style=\"border:1px solid #bbb;padding:2px 3px;background:#f8f8f8;color:{stroke}\">{ci}</th>"
                        ));
                    }
                    tbl.push_str("</tr></thead><tbody>");

                    for row in (0..n_rows).rev() {
                        let row_cells: Vec<String> = (0..n_cols)
                            .map(|c| pdf_utils::cell_text(&cells, row, c))
                            .collect();
                        if row_cells.iter().all(|s| s.trim().is_empty()) { continue; }

                        let is_isin = row_cells.iter().any(|s| isin_re().is_match(s.trim()));
                        let is_drv  = row_cells.iter().any(|s| deriv_prefix_re().is_match(s.trim()));
                        let row_bg = if is_isin { "background:#efffef" }
                                     else if is_drv { "background:#fffbea" }
                                     else { "" };
                        tbl.push_str(&format!(
                            "<tr style=\"{row_bg}\"><td style=\"border:1px solid #ccc;padding:2px 4px;color:#888\">{row}</td>"
                        ));
                        for cell in &row_cells {
                            let ct = cell.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
                            let cbg = if !ct.trim().is_empty() { "background:rgba(255,255,220,0.5);" } else { "" };
                            tbl.push_str(&format!("<td style=\"border:1px solid #ddd;padding:2px 3px;{cbg}\">{ct}</td>"));
                        }
                        tbl.push_str("</tr>");
                    }
                    tbl.push_str("</tbody></table>");

                    tables_html.push_str(&format!(
                        "<div style=\"margin-top:10px\"><h4 style=\"margin:4px 0;color:{label_clr}\">band {bi} — {} cols × {} qualifying rows (territory y=[{:.0},{:.0}])</h4>{tbl}</div>",
                        n_cols,
                        {
                            let mut cnt = 0;
                            for row in 0..n_rows {
                                let row_cells: Vec<String> = (0..n_cols).map(|c| pdf_utils::cell_text(&cells, row, c)).collect();
                                if row_cells.iter().any(|s| !s.trim().is_empty()) { cnt += 1; }
                            }
                            cnt
                        },
                        terr_lo, terr_hi.min(page_h),
                    ));
                }

                full_body.push_str(&format!(
                    "<div style=\"margin-bottom:48px\"><h3 style=\"margin:0 0 4px\">{label} — Page {} (PDF#{pn}) — {} band(s)</h3>{svg}{tables_html}</div>",
                    pi + 1, n_bands,
                ));
            }
        }

        let html = format!(
            r#"<!DOCTYPE html>
<html><head><meta charset="utf-8">
<title>CN Banded Cell Data</title>
<style>body{{margin:0;padding:20px;background:#f5f5f5;font-family:sans-serif}}</style>
</head><body>
<h2 style="margin:0 0 4px">Choice Equity CN — Banded Cell Data</h2>
<p style="margin:0 0 16px;font-size:13px">
  Each coloured region = one table band's territory (column headers + data rows).<br>
  Dashed lines = detected V-line extents (column header section only).<br>
  Green rows = equity (ISIN detected) &nbsp; Yellow rows = derivative (FUTSTK/OPTSTK/… detected)
</p>
{full_body}
</body></html>"#
        );

        let out = r"C:\Users\SUMIT\AppData\Local\Temp\cn_banded_data.html";
        std::fs::write(out, &html).expect("write HTML");
        println!("\n✓  Saved to: {out}\n   Open in any browser.\n");
    }
}
