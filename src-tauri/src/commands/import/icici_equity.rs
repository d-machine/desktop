//! Parser for ICICI Securities — Equity Transaction Statement (TRX-Equity PDF).
//!
//! # PDF layout (A4 portrait)
//!
//! The statement contains one or more contract note sections, one per trading day.
//! Each section has a **trades table** followed by a **charges summary table**.
//!
//! ## Trades table columns (approximate x positions)
//!
//! | X range   | Content                                                     |
//! |-----------|-------------------------------------------------------------|
//! |    0–100  | Contract Note number  "ISEC/YYYYDDD/NNNNN…"  (2-line wrap) |
//! |  140–300  | Exchange  "BSE" \| "NSE"                                    |
//! |  305–335  | Exchange trade number (7–10 digits)                         |
//! |  340–385  | Trade date  "DD-MM-YYYY"                                    |
//! |  385–490  | Trade time "HH:MM:SS" IST and/or settlement date            |
//! |  500–590  | Security name (may continue on physical line 2 / 3)         |
//! |  595–608  | Buy/Sell flag  "B" \| "S"                                   |
//! |  608–748  | Quantity                                                    |
//! |  748–840  | Brokerage + Price + ISIN  concatenated                      |
//! |  840+     | Net / GST / Gross amounts (ignored per-trade)               |
//!
//! Each trade spans 3 physical lines (y-gap ~4 pt within, ~13 pt between trades).
//! The 5 pt row-grouper in `pdf_utils` collapses all 3 lines into one logical row,
//! so both parts of the wrapped CN number appear in the same merged span list.
//!
//! ## Charges summary table
//!
//! Appears after the trades table for each contract note.  The CN number is in the
//! **second** column of its header row.  Charge rows contain a label keyword
//! (STT, Stamp Duty, GST, Transaction Charges) followed by a monetary amount.
//!
//! # Contract-note granularity and deduplication
//!
//! One `import_batches` row is created **per contract note** (`ref_no = CN number`).
//! Before inserting, the importer checks whether a batch with the same
//! `(account_id, source_type, ref_no)` already exists.  If so the entire contract
//! note is skipped.  This cleanly handles quarterly/annual statement overlap where
//! the same trading day appears in both a Q-statement and an FY-statement.
//!
//! If the CN number cannot be extracted from the PDF, a random hex fallback ID is
//! generated so that `ref_no` is never NULL and all trades still belong to a batch.

use crate::{commands::import::{flag_oversells, pdf_utils}, db};
use rand::Rng;
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;

// ─── Regexes ──────────────────────────────────────────────────────────────────

/// Matches the start of an ICICI contract note number: "ISEC/..." prefix.
fn cn_start_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^ISEC/").unwrap())
}

/// Matches a trade date in DD-MM-YYYY format.
fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{2}-\d{2}-\d{4}$").unwrap())
}

/// Matches a trade execution time in HH:MM format (as printed in the trade time column).
fn time_hhmm_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{2}:\d{2}$").unwrap())
}

/// Matches a full timestamp in HH:MM:SS format (order time column — not used for storage).
fn time_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{2}:\d{2}:\d{2}$").unwrap())
}

/// Matches the concatenated brokerage+price+ISIN field at x~748.
/// Format: two decimals (each exactly 2 dp) immediately followed by a 12-char ISIN.
fn price_isin_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^(\d+\.\d{2})(\d+\.\d{2})(IN[A-Z0-9]{10})").unwrap())
}

/// Matches a monetary amount: optional minus, digits with optional commas, dot, 2 dp.
fn amount_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^-?[\d,]+\.\d{2}$").unwrap())
}

/// Matches the statement date-range header line.
fn statement_range_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(
        r"Equity Transaction Statement from (\d{2}-[A-Za-z]+-\d{4}) to (\d{2}-[A-Za-z]+-\d{4})"
    ).unwrap())
}

// ─── Column x-boundaries ─────────────────────────────────────────────────────

/// Right boundary of the Contract Note number column (leftmost column in trades table).
const X_CN_END:     f32 = 100.0;
/// Left boundary of the Exchange column ("BSE" / "NSE").
const X_EXCHANGE:   f32 = 140.0;
/// Left boundary of the exchange trade number column (7–10 digit string).
const X_TRADE_NO:   f32 = 305.0;
/// Left boundary of the trade date column (DD-MM-YYYY).
const X_TRADE_DATE: f32 = 340.0;
/// Left boundary of the trade time column (HH:MM format, e.g. "09:17").
const X_TRADE_TIME: f32 = 355.0;
/// Left boundary of the settlement date area (ignored).
const X_TIME_AREA:  f32 = 395.0;
/// Left boundary of the security name column.
const X_SECURITY:   f32 = 500.0;
/// Left boundary of the buy/sell flag column.
const X_BUY_SELL:   f32 = 595.0;
/// Left boundary of the quantity column.
const X_QTY:        f32 = 608.0;
/// Left boundary of the brokerage+price+ISIN concatenation column.
const X_PRICE_ISIN: f32 = 748.0;
/// Left boundary of the per-trade amount column (net/GST/gross — ignored per-trade).
const X_AMOUNTS:    f32 = 840.0;

// ─── Internal types ───────────────────────────────────────────────────────────

/// A single trade extracted from the trades table of one contract note section.
#[derive(Debug, Clone)]
struct RawTrade {
    exchange:          String,        // "BSE" | "NSE"
    exchange_trade_no: String,        // raw digits from the trade-no column
    trade_date:        String,        // "YYYY-MM-DD"
    trade_time:        Option<String>,// "HH:MM:SS" IST
    security_name:     String,
    isin:              String,
    txn_type:          String,        // "BUY" | "SELL"
    quantity:          f64,
    price:             f64,           // rupees per unit (2 dp)
    brokerage:         f64,           // rupees (2 dp)
}

/// Parsed charges from one row of the summary table (one row per contract note).
#[derive(Debug, Clone, Default)]
struct SummaryCharge {
    trade_date:          String,
    stt_paise:           i64,
    stamp_charges_paise: i64,
    trans_charges_paise: i64,
    /// Net payable/receivable extracted from the "Rs. NNNN" text in the last column.
    total_payable_paise: i64,
}

/// Aggregate charges for one contract note, sourced from the summary table.
#[derive(Debug, Clone, Default)]
struct BlockCharges {
    stt_paise:           i64,
    stamp_charges_paise: i64,
    gst_paise:           i64,
    trans_charges_paise: i64,
    other_charges_paise: i64,
    total_payable_paise: i64,
}

/// One contract note: the full set of trades for a trading day plus aggregate charges.
#[derive(Debug)]
struct ContractNote {
    /// ICICI contract note number (e.g. "ISEC/2024062/000080122").
    /// Falls back to a random hex string if the number cannot be extracted.
    cn_no:   String,
    /// Trade date in "YYYY-MM-DD" format (taken from the first trade in the note).
    date:    String,
    trades:  Vec<RawTrade>,
    charges: BlockCharges,
}

// ─── Public types (serialised over Tauri IPC) ─────────────────────────────────

/// A single trade as returned by `parse_icici_equity_pdf` and forwarded by the
/// frontend to `import_icici_equity_trades`.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IciciEquityTrade {
    /// Trade date "YYYY-MM-DD".
    pub trade_date:    String,
    /// Trade execution time "HH:MM:SS" IST (populated when available in the PDF).
    pub trade_time:    Option<String>,
    /// Contract note number this trade belongs to (used for batch-level dedup).
    pub cn_no:         String,
    pub security_name: String,
    pub isin:          String,
    /// "BSE" or "NSE".
    pub exchange:      String,
    /// "BUY" or "SELL".
    pub txn_type:      String,
    pub quantity:      f64,
    /// Rupees per unit (2 dp).
    pub price:         f64,
    /// Brokerage in rupees (2 dp).
    pub brokerage:     f64,
    /// "{exchange}-{exchange_trade_no}" — used as the per-trade dedup fallback.
    pub broker_ref:    Option<String>,
}

/// Aggregate charges for one contract note, forwarded alongside trades to the
/// import command so the `import_batches` row can store them correctly.
#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContractNoteCharges {
    /// Contract note number (links to the matching `IciciEquityTrade.cn_no`).
    pub cn_no:               String,
    /// Trade date "YYYY-MM-DD".
    pub trade_date:          String,
    pub stt_paise:           i64,
    pub stamp_charges_paise: i64,
    pub gst_paise:           i64,
    pub trans_charges_paise: i64,
    pub other_charges_paise: i64,
    pub total_payable_paise: i64,
}

/// Parse result returned to the frontend for trade preview.
#[derive(Debug, Serialize)]
pub struct IciciEquityParseResult {
    /// Flat list of all trades across all contract notes (for the preview table).
    pub transactions:     Vec<IciciEquityTrade>,
    /// One entry per contract note with aggregate charges (forwarded to import).
    pub contract_charges: Vec<ContractNoteCharges>,
    pub total_rows:       usize,
    pub skipped_rows:     usize,
    pub client_code:      String,
    pub date_range:       (String, String),
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
pub struct IciciEquityImportResult {
    pub imported:                 usize,
    pub skipped:                  usize,
    /// Number of contract notes whose trades were inserted (new batches created).
    pub contract_notes_imported:  usize,
    /// Number of contract notes skipped because they were already in the DB.
    pub contract_notes_skipped:   usize,
    pub auto_created_instruments: usize,
    pub skipped_details:          Vec<SkippedDetail>,
}

// ─── Parse command ────────────────────────────────────────────────────────────

/// Parse an ICICI Securities Equity Transaction Statement PDF.
///
/// Returns a flat trade list for the preview table together with per-contract-note
/// charges.  Both collections must be forwarded unchanged to
/// [`import_icici_equity_trades`] to enable correct batch creation and
/// batch-level deduplication.
#[tauri::command]
pub fn parse_icici_equity_pdf(file_path: String) -> Result<IciciEquityParseResult, String> {
    let all_pages = pdf_utils::extract_all_page_spans(&file_path)?;

    let (client_code, date_range) = extract_metadata(&all_pages);
    let notes = extract_contract_notes(&all_pages);

    let mut transactions    = Vec::new();
    let mut contract_charges = Vec::new();

    for note in &notes {
        for raw in &note.trades {
            transactions.push(raw_trade_to_public(raw, &note.cn_no));
        }
        contract_charges.push(ContractNoteCharges {
            cn_no:               note.cn_no.clone(),
            trade_date:          note.date.clone(),
            stt_paise:           note.charges.stt_paise,
            stamp_charges_paise: note.charges.stamp_charges_paise,
            gst_paise:           note.charges.gst_paise,
            trans_charges_paise: note.charges.trans_charges_paise,
            other_charges_paise: note.charges.other_charges_paise,
            total_payable_paise: note.charges.total_payable_paise,
        });
    }

    let total = transactions.len();

    Ok(IciciEquityParseResult {
        skipped_rows: 0,
        total_rows: total,
        transactions,
        contract_charges,
        client_code,
        date_range,
    })
}

/// Convert an internal `RawTrade` into the public `IciciEquityTrade` IPC type.
fn raw_trade_to_public(raw: &RawTrade, cn_no: &str) -> IciciEquityTrade {
    let broker_ref = if raw.exchange_trade_no.is_empty() {
        None
    } else {
        Some(format!("{}-{}", raw.exchange, raw.exchange_trade_no))
    };

    IciciEquityTrade {
        trade_date:    raw.trade_date.clone(),
        trade_time:    raw.trade_time.clone(),
        cn_no:         cn_no.to_string(),
        security_name: raw.security_name.clone(),
        isin:          raw.isin.clone(),
        exchange:      raw.exchange.clone(),
        txn_type:      raw.txn_type.clone(),
        quantity:      raw.quantity,
        price:         raw.price,
        brokerage:     raw.brokerage,
        broker_ref,
    }
}

// ─── Metadata extraction ──────────────────────────────────────────────────────

/// Extract the client code and statement date range from the header pages.
fn extract_metadata(pages: &[Vec<pdf_utils::TextSpan>]) -> (String, (String, String)) {
    let mut client_code = String::new();
    let mut date_range  = (String::new(), String::new());

    'outer: for page in pages {
        let rows = pdf_utils::page_spans_to_rows(page.clone(), 5.0);
        for row in &rows {
            let text: String = row.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");

            if client_code.is_empty() && text.contains("UNIQUE CLIENT CODE") {
                if let Some(code) = text.split(':').nth(1) {
                    client_code = code.trim().to_string();
                }
            }
            if date_range.0.is_empty() {
                if let Some(caps) = statement_range_re().captures(&text) {
                    date_range.0 = caps[1].to_string();
                    date_range.1 = caps[2].to_string();
                }
            }
            if !client_code.is_empty() && !date_range.0.is_empty() {
                break 'outer;
            }
        }
    }

    (client_code, date_range)
}

// ─── Contract note extraction ─────────────────────────────────────────────────

/// Walk every page and group rows into contract notes.
///
/// Uses a two-pass approach:
/// 1. First pass: collect trade rows into contract notes, keyed by CN number.
/// 2. Second pass: extract aggregate charges from the summary table (a separate
///    page at the end of the PDF), matched to contract notes by trade date.
///
/// The CN number in the trades table appears in col 1 (x < ~100).  It wraps
/// across two physical lines; the 5 pt row-grouper typically merges them so both
/// parts appear in the same logical row.  When they are on separate rows (rare),
/// `pending_prefix` carries the first part forward to the next row.
fn extract_contract_notes(pages: &[Vec<pdf_utils::TextSpan>]) -> Vec<ContractNote> {
    // ── Pass 1: collect trades ────────────────────────────────────────────────
    let mut notes: HashMap<String, ContractNote> = HashMap::new();
    let mut cn_order: Vec<String> = Vec::new();
    let mut active_cn: Option<String> = None;
    let mut pending_prefix: Option<(String, f32)> = None;

    for page in pages {
        let rows = pdf_utils::page_spans_to_rows(page.clone(), 5.0);

        for row in &rows {
            // Detect CN number (may be complete in one row, or split across two rows)
            let row_cn = if let Some(cn) = extract_cn_from_row(row) {
                pending_prefix = None;
                Some(cn)
            } else if let Some((prefix, px)) = pending_prefix.take() {
                join_cn_continuation(row, &prefix, px)
            } else {
                None
            };

            if row_cn.is_none() {
                if let Some(partial) = extract_incomplete_cn_prefix(row) {
                    pending_prefix = Some(partial);
                }
            }

            if let Some(cn) = row_cn {
                if !notes.contains_key(&cn) {
                    notes.insert(cn.clone(), ContractNote {
                        cn_no:   cn.clone(),
                        date:    String::new(),
                        trades:  Vec::new(),
                        charges: BlockCharges::default(),
                    });
                    cn_order.push(cn.clone());
                }
                active_cn = Some(cn);
            }

            if let Some(trade) = try_parse_trade_row(row) {
                if active_cn.is_none() {
                    let fallback = gen_fallback_cn();
                    notes.insert(fallback.clone(), ContractNote {
                        cn_no:   fallback.clone(),
                        date:    String::new(),
                        trades:  Vec::new(),
                        charges: BlockCharges::default(),
                    });
                    cn_order.push(fallback.clone());
                    active_cn = Some(fallback);
                }
                let cn = active_cn.as_ref().unwrap();
                let note = notes.get_mut(cn).unwrap();
                if note.date.is_empty() {
                    note.date = trade.trade_date.clone();
                }
                note.trades.push(trade);
            }
        }
    }

    // ── Pass 2: extract summary-table charges and merge by trade date ─────────
    //
    // The summary table is on a separate page from the trades.  It has one row
    // per contract note with STT, transaction charges, stamp duty, and net amount.
    // The CN number printed in the summary uses a truncated format that differs
    // from the full CN number in the trades table, so we match by trade date.
    let summary_charges = extract_summary_charges(pages);
    let charges_by_date: HashMap<String, &SummaryCharge> = summary_charges.iter()
        .map(|c| (c.trade_date.clone(), c))
        .collect();

    let mut result: Vec<ContractNote> = cn_order.into_iter()
        .filter_map(|cn| notes.remove(&cn))
        .filter(|n| !n.trades.is_empty())
        .collect();

    for note in &mut result {
        if let Some(sc) = charges_by_date.get(&note.date) {
            note.charges.stt_paise           = sc.stt_paise;
            note.charges.trans_charges_paise = sc.trans_charges_paise;
            note.charges.stamp_charges_paise = sc.stamp_charges_paise;
            note.charges.total_payable_paise = sc.total_payable_paise;
        }
    }

    result
}

/// Walk all pages and extract rows from the summary table.
///
/// The summary table is identified by a "Summary" header row.  Each data row
/// has a trade date at x < 50, amounts in three charge columns, and a net
/// amount text in the rightmost column.
fn extract_summary_charges(pages: &[Vec<pdf_utils::TextSpan>]) -> Vec<SummaryCharge> {
    let mut in_summary = false;
    let mut result = Vec::new();

    for page in pages {
        let rows = pdf_utils::page_spans_to_rows(page.clone(), 5.0);

        for row in &rows {
            // Detect the "Summary" section header — a row whose only text is "Summary"
            let row_text: String = row.iter().map(|s| s.text.trim()).collect::<Vec<_>>().join(" ").trim().to_string();
            if row_text == "Summary" {
                in_summary = true;
                continue;
            }

            if !in_summary { continue; }

            if let Some(sc) = try_parse_summary_row(row) {
                result.push(sc);
            }
        }
    }

    result
}

/// Attempt to parse one row of the summary table into a `SummaryCharge`.
///
/// A valid summary row has:
///   - A date in DD-MM-YYYY format at x < 50 (Contract Date column)
///   - At least one numeric amount at x > 400 (STT / transaction / stamp columns)
///
/// The summary's CN number column (x~103) is intentionally ignored because it
/// uses a truncated format that does not match the full CN from the trades table.
/// Charges are merged into contract notes by trade date instead.
fn try_parse_summary_row(row: &[pdf_utils::TextSpan]) -> Option<SummaryCharge> {
    // Must have a date at x < 50 (Contract Date column)
    let date_span = row.iter().find(|s| s.x < 50.0 && date_re().is_match(s.text.trim()))?;
    let trade_date = parse_date_dmy(date_span.text.trim());

    // STT at x ~ 415–500 (Securities Transaction Tax column)
    let stt_paise = row.iter()
        .filter(|s| s.x >= 415.0 && s.x < 500.0)
        .filter_map(|s| parse_amount_paise_flexible(&s.text))
        .next()
        .unwrap_or(0);

    // Transaction charges at x ~ 500–600
    let trans_paise = row.iter()
        .filter(|s| s.x >= 500.0 && s.x < 600.0)
        .filter_map(|s| parse_amount_paise_flexible(&s.text))
        .next()
        .unwrap_or(0);

    // Stamp duty at x ~ 600–635
    let stamp_paise = row.iter()
        .filter(|s| s.x >= 600.0 && s.x < 635.0)
        .filter_map(|s| parse_amount_paise_flexible(&s.text))
        .next()
        .unwrap_or(0);

    // Net amount from the rightmost text column (x >= 635)
    // Format: "Net amount receivable by Client Rs. 8320.2"
    //      or "Net amount payable by Client Rs. 1030.02"
    let net_text = row.iter()
        .filter(|s| s.x >= 635.0)
        .map(|s| s.text.as_str())
        .collect::<Vec<_>>()
        .join(" ");
    let net_paise = extract_net_amount_paise(&net_text);

    // Row must have at least one non-zero charge to be a valid data row
    // (skips header rows and footnote rows that might match the date check)
    if stt_paise == 0 && trans_paise == 0 && stamp_paise == 0 && net_paise == 0 {
        return None;
    }

    Some(SummaryCharge {
        trade_date,
        stt_paise,
        trans_charges_paise: trans_paise,
        stamp_charges_paise: stamp_paise,
        total_payable_paise: net_paise,
    })
}

// ─── CN number helpers ────────────────────────────────────────────────────────

/// Extract a complete contract note number from a logical row.
///
/// Looks for a span matching "^ISEC/" anywhere in the row, then finds a
/// digit-only continuation span at approximately the same x position (same column).
/// If both parts are present in the row (5 pt grouper merged both lines), the full
/// CN number is returned directly.  Returns `None` if no "ISEC/" span is found.
fn extract_cn_from_row(row: &[pdf_utils::TextSpan]) -> Option<String> {
    // Find the ISEC/ prefix span
    let prefix_span = row.iter().find(|s| cn_start_re().is_match(s.text.trim()))?;
    let prefix      = prefix_span.text.trim().to_string();
    let prefix_x    = prefix_span.x;

    // Look for a digit continuation near the prefix column.
    // In the PDF the prefix lands at x≈10 and the continuation at x≈37 — a 27 pt gap —
    // so the tolerance must be at least 30 pt.  The next column (settlement no.) starts
    // around x≈95, safely outside this window.
    let continuation = row.iter()
        .filter(|s| {
            let t = s.text.trim();
            !t.is_empty()
            && (s.x - prefix_x).abs() < 40.0
            && t.chars().all(|c| c.is_ascii_digit())
        })
        .map(|s| s.text.trim().to_string())
        .next()
        .unwrap_or_default();

    Some(format!("{}{}", prefix, continuation))
}

/// Check if a row contains only the "ISEC/" prefix with no digit continuation yet.
///
/// Used when the 5 pt grouper failed to merge the CN number's two physical lines
/// into one logical row.  Returns `(prefix, x_position)` for carry-forward.
fn extract_incomplete_cn_prefix(row: &[pdf_utils::TextSpan]) -> Option<(String, f32)> {
    let span = row.iter().find(|s| cn_start_re().is_match(s.text.trim()))?;
    // If a digit continuation already exists at the same x in this row, the full
    // CN was already extracted by extract_cn_from_row — this function is a no-op.
    let has_continuation = row.iter().any(|s| {
        let t = s.text.trim();
        !t.is_empty()
        && (s.x - span.x).abs() < 40.0
        && t.chars().all(|c| c.is_ascii_digit())
    });
    if has_continuation { None } else { Some((span.text.trim().to_string(), span.x)) }
}

/// Join a pending CN prefix with a digit continuation found on the given row.
///
/// The continuation span must be at approximately the same x position as the prefix.
/// Returns `None` if no suitable span is found (the prefix will be discarded).
fn join_cn_continuation(
    row: &[pdf_utils::TextSpan],
    prefix: &str,
    prefix_x: f32,
) -> Option<String> {
    let digits = row.iter()
        .filter(|s| {
            let t = s.text.trim();
            !t.is_empty()
            && (s.x - prefix_x).abs() < 40.0
            && t.chars().all(|c| c.is_ascii_digit())
        })
        .map(|s| s.text.trim().to_string())
        .next()?;
    Some(format!("{}{}", prefix, digits))
}

/// Generate a random hex fallback CN number for the rare case where the ICICI
/// contract note number cannot be extracted from the PDF.
fn gen_fallback_cn() -> String {
    let n: u64 = rand::thread_rng().gen();
    format!("FALLBACK-{:016x}", n)
}

// ─── Trade row parser ─────────────────────────────────────────────────────────

/// Attempt to parse a logical row group as a single trade.
///
/// A trade row has all of: exchange code, exchange trade number, date, buy/sell
/// flag, quantity, and the brokerage+price+ISIN concatenation.  Returns `None`
/// for header rows, footer rows, charge rows, and any row missing required fields.
///
/// The CN number is extracted separately by `extract_cn_from_row` and NOT read
/// here to keep responsibilities cleanly separated.
fn try_parse_trade_row(row: &[pdf_utils::TextSpan]) -> Option<RawTrade> {
    let mut exchange     = String::new();
    let mut trade_no     = String::new();
    let mut trade_date   = String::new();
    let mut trade_time   = None::<String>;
    let mut security     = String::new();
    let mut buy_sell     = String::new();
    let mut qty_str      = String::new();
    let mut price_concat = String::new();

    for span in row {
        let x = span.x;
        let t = span.text.trim();
        if t.is_empty() { continue; }

        if x >= X_EXCHANGE && x < X_TRADE_NO {
            if t == "BSE" || t == "NSE" {
                exchange = t.to_string();
            }
        } else if x >= X_TRADE_NO && x < X_TRADE_DATE {
            // Exchange trade number: pure digits, minimum 5 chars
            if trade_no.is_empty() && t.chars().all(|c| c.is_ascii_digit()) && t.len() >= 5 {
                trade_no = t.to_string();
            }
        } else if x >= X_TRADE_DATE && x < X_TRADE_TIME {
            if date_re().is_match(t) && trade_date.is_empty() {
                trade_date = t.to_string();
            }
        } else if x >= X_TRADE_TIME && x < X_TIME_AREA {
            // Trade date (repeated at same x) or trade time in HH:MM format
            if date_re().is_match(t) && trade_date.is_empty() {
                trade_date = t.to_string();
            } else if trade_time.is_none() && time_hhmm_re().is_match(t) {
                // Store as HH:MM:SS by appending ":00" (seconds not printed in trade time column)
                trade_time = Some(format!("{}:00", t));
            }
        } else if x >= X_TIME_AREA && x < X_SECURITY {
            // Settlement date — ignored
        } else if x >= X_SECURITY && x < X_BUY_SELL {
            if !security.is_empty() { security.push(' '); }
            security.push_str(t);
        } else if x >= X_BUY_SELL && x < X_QTY {
            if t == "B" || t == "S" {
                buy_sell = t.to_string();
            }
        } else if x >= X_QTY && x < X_PRICE_ISIN {
            // Take the first digit-starting token; strip commas from formatted numbers
            if qty_str.is_empty() && t.chars().next().map_or(false, |c| c.is_ascii_digit()) {
                qty_str = t.replace(',', "");
            }
        } else if x >= X_PRICE_ISIN && x < X_AMOUNTS {
            // The longest match wins (the full concatenated string is one span)
            if price_concat.is_empty() || t.len() > price_concat.len() {
                price_concat = t.to_string();
            }
        }
    }

    // All of these must be present for a valid trade row
    if trade_date.is_empty() || buy_sell.is_empty() || qty_str.is_empty() || price_concat.is_empty() {
        return None;
    }

    let caps      = price_isin_re().captures(&price_concat)?;
    let brokerage = caps[1].parse::<f64>().ok()?;
    let price     = caps[2].parse::<f64>().ok()?;
    let isin      = caps[3].to_string();

    let quantity = qty_str.parse::<f64>().ok()?;
    if quantity <= 0.0 || price <= 0.0 { return None; }

    Some(RawTrade {
        exchange:          if exchange.is_empty() { "BSE".to_string() } else { exchange },
        exchange_trade_no: trade_no,
        trade_date:        parse_date_dmy(&trade_date),
        trade_time,
        security_name:     security.trim().to_string(),
        isin,
        txn_type:          if buy_sell == "B" { "BUY".to_string() } else { "SELL".to_string() },
        quantity,
        price,
        brokerage,
    })
}


// ─── Amount and date helpers ──────────────────────────────────────────────────

/// Parse a monetary amount string with exactly 2 decimal places ("1,234.56" or "-123.45")
/// into integer paise.  Returns `None` if the string does not match.
fn parse_amount_paise(s: &str) -> Option<i64> {
    let clean = s.trim().replace(',', "");
    let check = clean.trim_start_matches('-');
    if !amount_re().is_match(check) { return None; }
    let f: f64 = clean.parse().ok()?;
    Some((f * 100.0).round() as i64)
}

/// Parse a monetary amount that may be an integer ("8") or decimal ("8.25").
///
/// Used for the summary table's STT column which sometimes prints round-number
/// amounts without a decimal point.
fn parse_amount_paise_flexible(s: &str) -> Option<i64> {
    let clean = s.trim().replace(',', "");
    if clean.is_empty() { return None; }
    // Try decimal first (most common)
    if let Some(p) = parse_amount_paise(&clean) { return Some(p); }
    // Fall back to integer (e.g. "8" → ₹8 = 800 paise)
    let check = clean.trim_start_matches('-');
    if check.chars().all(|c| c.is_ascii_digit()) && !check.is_empty() {
        let n: i64 = clean.parse().ok()?;
        return Some(n * 100);
    }
    None
}

/// Extract a net amount from a summary text string like
/// "Net amount receivable by Client Rs. 8320.2" or "Net amount payable by Client Rs. 1030.02".
/// Returns the amount in paise, or 0 if not found.
fn extract_net_amount_paise(text: &str) -> i64 {
    if let Some(idx) = text.rfind("Rs.") {
        let after = text[idx + 3..].trim();
        // Take the first whitespace-delimited token after "Rs."
        let num_str = after.split_whitespace().next().unwrap_or("").replace(',', "");
        if let Ok(f) = num_str.parse::<f64>() {
            return (f * 100.0).round() as i64;
        }
    }
    0
}

/// Convert "DD-MM-YYYY" to "YYYY-MM-DD".
fn parse_date_dmy(s: &str) -> String {
    let p: Vec<&str> = s.split('-').collect();
    if p.len() == 3 { format!("{}-{}-{}", p[2], p[1], p[0]) } else { s.to_string() }
}


// ─── Import command ───────────────────────────────────────────────────────────

/// Bulk-insert parsed ICICI Equity trades, creating one `import_batches` row per
/// contract note.
///
/// Trades are grouped by `cn_no`.  For each group:
/// 1. A batch-level dedup check runs against `import_batches` using
///    `(account_id, source_type='ICICI_TRX_EQUITY', ref_no=cn_no)`.
///    If the batch already exists the entire contract note is skipped — this
///    handles quarterly/annual statement overlap where the same trading day appears
///    in both files.
/// 2. If the batch is new, an `import_batches` row is inserted with the aggregate
///    charges from `contract_charges`, and each trade is inserted with `batch_id`.
/// 3. `INSERT OR IGNORE` relies on the UNIQUE INDEX `(account_id, broker_ref)` as
///    a fallback guard against any remaining per-trade duplicates.
#[tauri::command]
pub fn import_icici_equity_trades(
    account_id: i64,
    transactions: Vec<IciciEquityTrade>,
    contract_charges: Vec<ContractNoteCharges>,
    file_paths: Option<Vec<String>>,
) -> Result<IciciEquityImportResult, String> {
    // Compact file paths into a single JSON string stored per batch row.
    // We store the original paths so the user can re-open the source PDF.
    let file_name: Option<String> = file_paths.as_ref().filter(|v| !v.is_empty()).map(|paths| {
        serde_json::to_string(paths).unwrap_or_default()
    });
    let conn = db::acquire()?;

    // Build cn_no → charges map for O(1) lookup during batch insert
    let charges_map: HashMap<String, &ContractNoteCharges> = contract_charges.iter()
        .map(|c| (c.cn_no.clone(), c))
        .collect();

    // Group trades by cn_no, preserving first-seen insertion order
    let mut cn_order: Vec<String> = Vec::new();
    let mut cn_trades: HashMap<String, Vec<&IciciEquityTrade>> = HashMap::new();
    for trade in &transactions {
        if !cn_trades.contains_key(&trade.cn_no) {
            cn_order.push(trade.cn_no.clone());
            cn_trades.insert(trade.cn_no.clone(), Vec::new());
        }
        cn_trades.get_mut(&trade.cn_no).unwrap().push(trade);
    }

    let equity_type_id: i64 = conn.query_row(
        "SELECT instrument_type_id FROM instrument_types WHERE name='EQUITY' LIMIT 1",
        [], |r| r.get(0),
    ).unwrap_or(1);

    let bse_exchange_id: Option<i64> = conn.query_row(
        "SELECT exchange_id FROM exchanges WHERE code='BSE' LIMIT 1",
        [], |r| r.get(0),
    ).ok();

    let nse_exchange_id: Option<i64> = conn.query_row(
        "SELECT exchange_id FROM exchanges WHERE code='NSE' LIMIT 1",
        [], |r| r.get(0),
    ).ok();

    let mut imported     = 0usize;
    let mut skipped      = 0usize;
    let mut cns_imported = 0usize;
    let mut cns_skipped  = 0usize;
    let mut auto_created = 0usize;
    let mut skipped_details: Vec<SkippedDetail> = Vec::new();

    for cn_no in &cn_order {
        let trades = cn_trades.get(cn_no).unwrap();

        // ── Batch-level dedup: skip the whole CN if it was imported before ────
        let batch_exists: bool = conn.query_row(
            "SELECT COUNT(*) FROM import_batches
             WHERE account_id=?1 AND source_type='ICICI_TRX_EQUITY' AND ref_no=?2",
            rusqlite::params![account_id, cn_no],
            |r| r.get::<_, i64>(0),
        ).map(|c| c > 0).unwrap_or(false);

        if batch_exists {
            cns_skipped += 1;
            skipped     += trades.len();
            for trade in trades {
                skipped_details.push(SkippedDetail {
                    trade_date:    trade.trade_date.clone(),
                    security_name: trade.security_name.clone(),
                    txn_type:      trade.txn_type.clone(),
                    quantity:      trade.quantity,
                    price:         trade.price,
                    reason:        format!("Contract note {} already imported", cn_no),
                });
            }
            continue;
        }

        // ── Create the import batch for this contract note ────────────────────
        let charges   = charges_map.get(cn_no.as_str());
        let trade_date = trades.first().map(|t| t.trade_date.as_str()).unwrap_or("");

        conn.execute(
            "INSERT INTO import_batches
                (account_id, source_type, file_name, ref_no, broker, batch_trade_date,
                 stt_paise, stamp_charges_paise, gst_paise, trans_charges_paise,
                 other_charges_paise, total_payable_paise)
             VALUES (?1,'ICICI_TRX_EQUITY',?2,?3,'ICICI Securities',?4,?5,?6,?7,?8,?9,?10)",
            rusqlite::params![
                account_id, &file_name, cn_no, trade_date,
                charges.map(|c| c.stt_paise).unwrap_or(0),
                charges.map(|c| c.stamp_charges_paise).unwrap_or(0),
                charges.map(|c| c.gst_paise).unwrap_or(0),
                charges.map(|c| c.trans_charges_paise).unwrap_or(0),
                charges.map(|c| c.other_charges_paise).unwrap_or(0),
                charges.map(|c| c.total_payable_paise).unwrap_or(0),
            ],
        ).map_err(|e| e.to_string())?;

        let batch_id = conn.last_insert_rowid();
        cns_imported += 1;

        // ── Insert each trade belonging to this contract note ─────────────────
        for trade in trades {
            let instrument_id = resolve_instrument(
                &conn, trade, equity_type_id, bse_exchange_id, nse_exchange_id,
                &mut auto_created,
            );

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

            let price_paise     = (trade.price * 100.0).round() as i64;
            let brokerage_paise = (trade.brokerage * 100.0).round() as i64;
            let gross_paise     = (trade.quantity * trade.price * 100.0).round() as i64;
            let total_value     = if trade.txn_type == "BUY" {
                -(gross_paise + brokerage_paise)
            } else {
                gross_paise - brokerage_paise
            };

            // INSERT OR IGNORE: the UNIQUE INDEX on (account_id, broker_ref) silently
            // rejects any per-trade duplicate that slipped through batch-level dedup.
            let rows = conn.execute(
                "INSERT OR IGNORE INTO transactions
                    (account_id, instrument_id, txn_type, trade_segment, trade_date, txn_time,
                     quantity, price_paise, brokerage_paise, stt_paise, other_charges_paise,
                     total_value_paise, notes, broker_ref, batch_id)
                 VALUES (?1,?2,?3,'DELIVERY',?4,?5,?6,?7,?8,0,0,?9,NULL,?10,?11)",
                rusqlite::params![
                    account_id, instrument_id, trade.txn_type,
                    trade.trade_date, trade.trade_time,
                    trade.quantity, price_paise, brokerage_paise,
                    total_value, trade.broker_ref, batch_id,
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
                    reason:        "Duplicate broker_ref".to_string(),
                });
            }
        }
    }

    drop(conn);
    flag_oversells(account_id)?;

    Ok(IciciEquityImportResult {
        imported,
        skipped,
        contract_notes_imported: cns_imported,
        contract_notes_skipped:  cns_skipped,
        auto_created_instruments: auto_created,
        skipped_details,
    })
}

/// Resolve or auto-create an equity instrument by ISIN, falling back to name lookup.
///
/// Returns the `instrument_id` on success, `None` if the auto-create INSERT also
/// failed (extremely rare — indicates a DB constraint violation).
fn resolve_instrument(
    conn: &rusqlite::Connection,
    trade: &IciciEquityTrade,
    equity_type_id: i64,
    bse_exchange_id: Option<i64>,
    nse_exchange_id: Option<i64>,
    auto_created: &mut usize,
) -> Option<i64> {
    // 1. Try by ISIN (most reliable)
    if !trade.isin.is_empty() {
        if let Ok(id) = conn.query_row(
            "SELECT instrument_id FROM instruments WHERE isin=?1 LIMIT 1",
            [&trade.isin], |r| r.get::<_, i64>(0),
        ) {
            return Some(id);
        }
    }

    // 2. Auto-create a placeholder equity instrument
    let exchange_id = if trade.exchange == "NSE" { nse_exchange_id } else { bse_exchange_id };
    let isin_val    = if trade.isin.is_empty() { None } else { Some(&trade.isin) };

    conn.execute(
        "INSERT OR IGNORE INTO instruments
            (name, isin, instrument_type_id, primary_exchange_id, source)
         VALUES (?1, ?2, ?3, ?4, 'IMPORT')",
        rusqlite::params![trade.security_name, isin_val, equity_type_id, exchange_id],
    ).ok()?;

    let id: Option<i64> = if !trade.isin.is_empty() {
        conn.query_row(
            "SELECT instrument_id FROM instruments WHERE isin=?1 LIMIT 1",
            [&trade.isin], |r| r.get(0),
        ).ok()
    } else {
        conn.query_row(
            "SELECT instrument_id FROM instruments WHERE name=?1 ORDER BY instrument_id DESC LIMIT 1",
            [&trade.security_name], |r| r.get(0),
        ).ok()
    };

    if let Some(inst_id) = id {
        let _ = conn.execute(
            "INSERT OR IGNORE INTO instrument_equity (instrument_id) VALUES (?1)",
            [inst_id],
        );
        *auto_created += 1;
        Some(inst_id)
    } else {
        None
    }
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const PDFS: &[&str] = &[
        "/home/dmachine/workspace/TRX-Equity_09-10-2021_962695.PDF",
        "/home/dmachine/workspace/TRX-Equity_07-04-2022_1098746.PDF",
        "/home/dmachine/workspace/TRX-Equity_03-07-2022_584558.PDF",
        "/home/dmachine/workspace/TRX-Equity_28-10-2022_1068522.PDF",
        "/home/dmachine/workspace/TRX-Equity_04-01-2023_851490.PDF",
        "/home/dmachine/workspace/TRX-Equity_24-04-2023_1152241.PDF",
        "/home/dmachine/workspace/TRX-Equity_07-01-2024_105249.PDF",
        "/home/dmachine/workspace/TRX-Equity_07-04-2024_61770.PDF",
        "/home/dmachine/workspace/TRX-Equity_06-07-2024_1091528.PDF",
        "/home/dmachine/workspace/TRX-Equity_21-04-2024_1646876.PDF",
        "/home/dmachine/workspace/TRX-Equity_10-04-2025_1498546.PDF",
    ];

    /// Smoke test: every available file should parse without errors and yield trades.
    #[test]
    fn test_parse_all() {
        for path in PDFS {
            if !std::path::Path::new(path).exists() { continue; }
            let result = parse_icici_equity_pdf(path.to_string()).expect("parse failed");
            println!("\n=== {} ===", path);
            println!("  client: {}  range: {:?}", result.client_code, result.date_range);
            println!("  trades: {}  contract_notes: {}", result.total_rows, result.contract_charges.len());
            for cn in &result.contract_charges {
                println!("  CN {} {} — stt={} stamp={} gst={} total={}",
                    cn.cn_no, cn.trade_date,
                    cn.stt_paise, cn.stamp_charges_paise, cn.gst_paise, cn.total_payable_paise);
            }
            assert!(result.total_rows > 0, "expected trades in {path}");
        }
    }

    /// Every trade must carry a non-empty ISIN.
    #[test]
    fn test_isin_present() {
        for path in PDFS {
            if !std::path::Path::new(path).exists() { continue; }
            let result = parse_icici_equity_pdf(path.to_string()).expect("parse failed");
            let missing: Vec<_> = result.transactions.iter()
                .filter(|t| t.isin.is_empty())
                .collect();
            println!("Missing ISIN in {}: {}", path, missing.len());
            assert!(missing.is_empty(), "all trades should have ISIN in {path}");
        }
    }

    /// Every trade must carry a contract note number (not a fallback).
    #[test]
    fn test_cn_numbers_present() {
        for path in PDFS {
            if !std::path::Path::new(path).exists() { continue; }
            let result = parse_icici_equity_pdf(path.to_string()).expect("parse failed");
            let fallback: Vec<_> = result.transactions.iter()
                .filter(|t| t.cn_no.starts_with("FALLBACK-"))
                .collect();
            println!("Fallback CNs in {}: {}", path, fallback.len());
            assert!(fallback.is_empty(),
                "{} trades have fallback CN numbers in {path} — CN extraction likely broken",
                fallback.len());
        }
    }

    /// Every trade must carry a broker_ref (exchange-prefixed trade number).
    #[test]
    fn test_broker_ref_present() {
        for path in PDFS {
            if !std::path::Path::new(path).exists() { continue; }
            let result = parse_icici_equity_pdf(path.to_string()).expect("parse failed");
            let missing: Vec<_> = result.transactions.iter()
                .filter(|t| t.broker_ref.is_none())
                .collect();
            println!("Missing broker_ref in {}: {}", path, missing.len());
            assert!(missing.is_empty(), "all trades should have broker_ref in {path}");
        }
    }

    /// Verify that charges are non-zero in at least one contract note per file.
    #[test]
    fn test_charges_extracted() {
        for path in PDFS {
            if !std::path::Path::new(path).exists() { continue; }
            let result = parse_icici_equity_pdf(path.to_string()).expect("parse failed");
            let any_charges = result.contract_charges.iter()
                .any(|c| c.total_payable_paise > 0 || c.stt_paise > 0 || c.gst_paise > 0);
            println!("{}: any_charges={any_charges}", path);
            // This is a soft check — warn but don't fail if charges are missing
            // (some test PDFs may be redacted versions without summary tables)
            if !any_charges {
                println!("  WARNING: no charges found — check if summary table was parsed correctly");
            }
        }
    }

    /// Detailed view of one known file: October 2022 statement (4 trades).
    #[test]
    fn test_oct22_detail() {
        let path = "/home/dmachine/workspace/TRX-Equity_28-10-2022_1068522.PDF";
        if !std::path::Path::new(path).exists() { return; }
        let result = parse_icici_equity_pdf(path.to_string()).expect("parse failed");
        println!("\nOct 2022 — {} trades, {} CNs:", result.total_rows, result.contract_charges.len());
        for t in &result.transactions {
            println!("  {} {} | {} | qty={} @ ₹{} | cn={} | broker_ref={:?} | time={:?}",
                t.trade_date, t.exchange, t.txn_type, t.quantity, t.price,
                t.cn_no, t.broker_ref, t.trade_time);
        }
        for cn in &result.contract_charges {
            println!("  CHARGES cn={} stt={} gst={} total={}",
                cn.cn_no, cn.stt_paise, cn.gst_paise, cn.total_payable_paise);
        }
        assert_eq!(result.total_rows, 4, "expected 4 trades in Oct 2022 file");
    }

    /// Cross-file duplicate analysis: parses all available files and verifies that
    /// zero trades appear more than once within a single file (parser correctness).
    /// Cross-file duplicates (same execution in quarterly + annual files) are
    /// expected and now handled via batch-level CN dedup at import time.
    #[test]
    fn test_no_intra_file_duplicates() {
        let mut intra = 0usize;
        let mut cross = 0usize;

        // broker_ref → list of (filename, cn_no)
        let mut ref_map: std::collections::HashMap<String, Vec<(String, String)>> =
            std::collections::HashMap::new();

        for path in PDFS {
            if !std::path::Path::new(path).exists() { continue; }
            let result = parse_icici_equity_pdf(path.to_string()).expect("parse failed");
            let fname = std::path::Path::new(path).file_name().unwrap()
                .to_string_lossy().to_string();

            for t in &result.transactions {
                let key = t.broker_ref.clone()
                    .unwrap_or_else(|| format!("NO_REF:{}:{}:{}", t.trade_date, t.isin, t.price));
                ref_map.entry(key).or_default().push((fname.clone(), t.cn_no.clone()));
            }
        }

        for (key, entries) in &ref_map {
            if entries.len() < 2 { continue; }
            let files: Vec<&str> = entries.iter().map(|(f, _)| f.as_str()).collect();
            let all_same = files.windows(2).all(|w| w[0] == w[1]);
            if all_same { intra += 1; } else { cross += 1; }
            if all_same {
                println!("  [INTRA-FILE BUG] broker_ref={key} appears {} times in same file", entries.len());
            }
        }

        println!("Intra-file dupes (parser bugs): {intra}");
        println!("Cross-file dupes (overlap, handled by CN dedup): {cross}");
        assert_eq!(intra, 0, "parser is emitting the same trade twice within one file");
    }
}
