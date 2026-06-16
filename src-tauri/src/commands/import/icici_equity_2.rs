//! ICICI Securities — Equity statement parser (border-based).
//!
//! Table structure is derived from the PDF's own graphical border lines via
//! `pdf_utils::extract_page_lines`, giving clean cell-based field access
//! without per-format X-coordinate tuning.

use crate::{commands::import::{common, flag_oversells, pdf_utils}, db};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::OnceLock;
// ─── Regexes ──────────────────────────────────────────────────────────────────

fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{2}-\d{2}-\d{4}$").unwrap())
}

fn time_hhmm_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\d{2}:\d{2}$").unwrap())
}

fn isin_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^IN[A-Z0-9]{10}$").unwrap())
}

fn amount_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^-?[\d,]+\.\d{2}$").unwrap())
}

fn statement_range_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(
        r"Equity Transaction Statement from (\d{2}-[A-Za-z]+-\d{4}) to (\d{2}-[A-Za-z]+-\d{4})"
    ).unwrap())
}

// ─── Extraction constants ─────────────────────────────────────────────────────

const X_GAP:       f32 = 6.0;
const CHAR_Y_TOL:  f32 = 2.0;
// Minimum line lengths to count as table borders (shorter lines are decorative).
const MIN_H_LEN:          f32 = 30.0;
const MIN_V_LEN:          f32 = 15.0;
// Full column borders are 45.8 pt tall; header sub-dividers are 40.8 pt.
// Any X position whose longest V-segment is below this threshold is a
// sub-divider that only appears in header cells — filter it out to get the
// stable 18-column layout shared by all ICICI equity PDF formats.
const FULL_BORDER_MIN_LEN: f32 = 44.0;
// Max coordinate distance to cluster near-identical border endpoints.
const CLUSTER_GAP: f32 = 3.0;
// Tolerance when assigning a span's X to a column boundary.
const COL_SNAP:    f32 = 2.0;

// ─── Column indices — trades table ───────────────────────────────────────────
// Header-row sub-dividers are filtered out before column assignment, leaving
// 18 stable boundaries common to all ICICI equity formats (2021/2022/2024/2025).

const COL_CN:        usize = 0;  // Contract Note "ISEC/YYYYDDD/NNNNN"
const COL_EXCHANGE:  usize = 2;  // "BSE" | "NSE"
const COL_TRADE_NO:  usize = 4;  // Exchange trade number
const COL_TRADE_DT:  usize = 6;  // Trade date "DD-MM-YYYY" and time "HH:MM"
const COL_ISIN:      usize = 8;  // 12-char ISIN starting with "IN"
const COL_SECURITY:  usize = 9;  // Security / company name
const COL_BS:        usize = 10; // "B" (buy) | "S" (sell)
const COL_QTY:       usize = 11; // Quantity (integer or decimal)
const COL_PRICE:     usize = 12; // Per-unit price in rupees (2 dp)
const COL_BROKERAGE: usize = 13; // Total brokerage in rupees (2 dp)

// ─── Column indices — summary table ──────────────────────────────────────────

const SUM_COL_DATE:  usize = 0;  // Contract date "DD-MM-YYYY"
const SUM_COL_CN:    usize = 1;  // Contract note number (used only for row detection)
const SUM_COL_STT:   usize = 7;  // Securities Transaction Tax (rupees)
const SUM_COL_TRANS: usize = 9;  // Transaction charges (rupees)
const SUM_COL_STAMP: usize = 10; // Stamp duty (rupees)
const SUM_COL_NET:   usize = 11; // "Net amount receivable/payable by Client Rs. XXXX" text

// ─── Internal types ───────────────────────────────────────────────────────────

#[derive(Debug, Clone)]
struct RawTrade {
    cn_no:             String,
    exchange:          String,
    exchange_trade_no: String,
    trade_date:        String,
    trade_time:        Option<String>,
    security_name:     String,
    isin:              String,
    txn_type:          String,
    quantity:          f64,
    price:             f64,
    brokerage:         f64,
}

#[derive(Debug, Clone, Default)]
struct SummaryCharge {
    trade_date:          String,
    stt_paise:           i64,
    stamp_charges_paise: i64,
    trans_charges_paise: i64,
    total_payable_paise: i64,
}

#[derive(Debug, Clone, Default)]
struct BlockCharges {
    stt_paise:           i64,
    stamp_charges_paise: i64,
    gst_paise:           i64,
    trans_charges_paise: i64,
    other_charges_paise: i64,
    total_payable_paise: i64,
}

#[derive(Debug)]
struct ContractNote {
    cn_no:   String,
    date:    String,
    trades:  Vec<RawTrade>,
    charges: BlockCharges,
}

// ─── Cell map ────────────────────────────────────────────────────────────────

// (row_index, col_index) → [(y, text), ...] sorted y descending (top-first reading order)
type CellMap = pdf_utils::CellMap;

/// Return the first span in a cell that matches `pattern`.
fn cell_find(map: &CellMap, row: usize, col: usize, pattern: &Regex) -> String {
    map.get(&(row, col))
       .and_then(|v| v.iter().find(|(_, t)| pattern.is_match(t.trim())))
       .map(|(_, t)| t.trim().to_string())
       .unwrap_or_default()
}

// ─── Public types (serialised over Tauri IPC) ─────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct IciciEquityTrade {
    pub trade_date:    String,
    pub trade_time:    Option<String>,
    pub cn_no:         String,
    pub security_name: String,
    pub isin:          String,
    pub exchange:      String,
    pub txn_type:      String,
    pub quantity:      f64,
    pub price:         f64,
    pub brokerage:     f64,
    pub broker_ref:    Option<String>,
}

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct ContractNoteCharges {
    pub cn_no:               String,
    pub trade_date:          String,
    pub stt_paise:           i64,
    pub stamp_charges_paise: i64,
    pub gst_paise:           i64,
    pub trans_charges_paise: i64,
    pub other_charges_paise: i64,
    pub total_payable_paise: i64,
}

#[derive(Debug, Serialize)]
pub struct IciciEquityParseResult {
    pub transactions:     Vec<IciciEquityTrade>,
    pub contract_charges: Vec<ContractNoteCharges>,
    pub total_rows:       usize,
    pub skipped_rows:     usize,
    pub client_code:      String,
    pub date_range:       (String, String),
    pub pages_with_text:  usize,
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
    pub contract_notes_imported:  usize,
    pub contract_notes_skipped:   usize,
    pub auto_created_instruments: usize,
    pub skipped_details:          Vec<SkippedDetail>,
}

// ─── Parse command ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn parse_icici_equity_pdf(file_path: String, password: Option<String>) -> Result<IciciEquityParseResult, String> {
    let doc = pdf_utils::load_pdf(&file_path, password.as_deref())?;

    let all_page_spans = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], X_GAP, CHAR_Y_TOL)?;
    let pages_with_text = all_page_spans.iter().filter(|p| !p.is_empty()).count();

    let (client_code, date_range) = extract_metadata(&all_page_spans);
    let notes = extract_contract_notes(&doc, &all_page_spans);

    let mut transactions     = Vec::new();
    let mut contract_charges = Vec::new();

    for note in &notes {
        for raw in &note.trades {
            transactions.push(raw_trade_to_public(raw));
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
        skipped_rows: 0, total_rows: total, transactions, contract_charges,
        client_code, date_range, pages_with_text,
    })
}

fn raw_trade_to_public(raw: &RawTrade) -> IciciEquityTrade {
    let broker_ref = if raw.exchange_trade_no.is_empty() {
        None
    } else {
        Some(format!("{}-{}", raw.exchange, raw.exchange_trade_no))
    };
    IciciEquityTrade {
        trade_date:    raw.trade_date.clone(),
        trade_time:    raw.trade_time.clone(),
        cn_no:         raw.cn_no.clone(),
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

fn extract_metadata(pages: &[Vec<pdf_utils::TextSpan>]) -> (String, (String, String)) {
    let mut client_code = String::new();
    let mut date_range  = (String::new(), String::new());

    'outer: for page in pages {
        let rows = pdf_utils::page_spans_to_rows(page.clone(), 5.0);
        for row in &rows {
            let text: String = row.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
            if client_code.is_empty() && text.contains("UNIQUE CLIENT CODE") {
                if let Some(code) = text.split(':').nth(1) {
                    if let Some(tok) = code.split_whitespace().next() {
                        client_code = tok.to_string();
                    }
                }
            }
            if date_range.0.is_empty() {
                if let Some(caps) = statement_range_re().captures(&text) {
                    date_range.0 = caps[1].to_string();
                    date_range.1 = caps[2].to_string();
                }
            }
            if !client_code.is_empty() && !date_range.0.is_empty() { break 'outer; }
        }
    }
    (client_code, date_range)
}

// ─── Contract note extraction ─────────────────────────────────────────────────

fn extract_contract_notes(
    doc:           &lopdf::Document,
    all_page_spans: &[Vec<pdf_utils::TextSpan>],
) -> Vec<ContractNote> {
    let pages: Vec<u32> = doc.get_pages().keys().copied().collect();

    let mut all_trades:    Vec<RawTrade>      = Vec::new();
    let mut all_summaries: Vec<SummaryCharge> = Vec::new();

    for (page_num, page_spans) in pages.iter().zip(all_page_spans.iter()) {
        let lines            = pdf_utils::extract_page_lines(doc, *page_num);
        let (row_ys, col_xs) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);

        // Drop header-row sub-dividers (40.8 pt) — keep only X positions that
        // have at least one full-height segment (≥ 44 pt). This collapses all
        // ICICI equity PDF formats to a stable 18-column layout.
        let col_xs: Vec<f32> = col_xs.into_iter()
            .filter(|&cx| lines.iter().any(|l| {
                l.is_vertical(1.0)
                    && (l.x1 - cx).abs() <= CLUSTER_GAP
                    && l.length() >= FULL_BORDER_MIN_LEN
            }))
            .collect();

        if col_xs.is_empty() || col_xs.len() <= SUM_COL_NET { continue; }

        let cells  = pdf_utils::build_cell_map(page_spans, &row_ys, &col_xs, COL_SNAP);
        let n_rows = row_ys.len() + 1;

        for row in 0..n_rows {
            let cn_text = pdf_utils::cell_text(&cells, row, COL_CN);
            if cn_text.contains("ISEC/") {
                if let Some(trade) = parse_trade_row(&cells, row, &cn_text) {
                    all_trades.push(trade);
                }
            }

            let sum_date = pdf_utils::cell_text(&cells, row, SUM_COL_DATE);
            let sum_cn   = pdf_utils::cell_text(&cells, row, SUM_COL_CN);
            if date_re().is_match(sum_date.trim()) && sum_cn.contains("ISEC/") {
                if let Some(sc) = parse_summary_row(&cells, row, &sum_date) {
                    all_summaries.push(sc);
                }
            }
        }
    }

    // Group trades by CN, preserving order
    let mut notes:    HashMap<String, ContractNote> = HashMap::new();
    let mut cn_order: Vec<String>                  = Vec::new();

    for trade in all_trades {
        let cn = trade.cn_no.clone();
        if !notes.contains_key(&cn) {
            notes.insert(cn.clone(), ContractNote {
                cn_no:   cn.clone(),
                date:    trade.trade_date.clone(),
                trades:  Vec::new(),
                charges: BlockCharges::default(),
            });
            cn_order.push(cn.clone());
        }
        notes.get_mut(&cn).unwrap().trades.push(trade);
    }

    // Match summary charges to contract notes by trade date
    let charges_by_date: HashMap<String, &SummaryCharge> =
        all_summaries.iter().map(|c| (c.trade_date.clone(), c)).collect();

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

// ─── Trade row parser ─────────────────────────────────────────────────────────

fn parse_trade_row(cells: &CellMap, row: usize, cn_text: &str) -> Option<RawTrade> {
    let exchange = pdf_utils::cell_text(cells, row, COL_EXCHANGE);
    if exchange != "BSE" && exchange != "NSE" { return None; }

    let isin = pdf_utils::cell_text(cells, row, COL_ISIN);
    if !isin_re().is_match(&isin) { return None; }

    let trade_date_str = cell_find(cells, row, COL_TRADE_DT, date_re());
    if trade_date_str.is_empty() { return None; }

    let trade_time = cell_find(cells, row, COL_TRADE_DT, time_hhmm_re());
    let trade_time = if trade_time.is_empty() { None } else { Some(format!("{trade_time}:00")) };

    let bs = pdf_utils::cell_text(cells, row, COL_BS);
    let txn_type = match bs.trim() {
        "B" => "BUY",
        "S" => "SELL",
        _   => return None,
    }.to_string();

    let qty = pdf_utils::cell_text(cells, row, COL_QTY).replace(',', "").parse::<f64>().ok()?;
    if qty <= 0.0 { return None; }

    let price = pdf_utils::cell_text(cells, row, COL_PRICE).replace(',', "").parse::<f64>().ok()?;
    if price <= 0.0 { return None; }

    let brokerage = pdf_utils::cell_text(cells, row, COL_BROKERAGE).replace(',', "").parse::<f64>().unwrap_or(0.0);

    let security = cells.get(&(row, COL_SECURITY))
        .map(|v| v.iter().map(|(_, t)| t.trim()).filter(|t| !t.is_empty()).collect::<Vec<_>>().join(" "))
        .unwrap_or_default()
        .trim().to_string();

    let trade_no = pdf_utils::cell_text(cells, row, COL_TRADE_NO);

    Some(RawTrade {
        cn_no:             cn_text.trim().to_string(),
        exchange:          exchange.trim().to_string(),
        exchange_trade_no: trade_no.trim().to_string(),
        trade_date:        parse_date_dmy(&trade_date_str),
        trade_time,
        security_name:     security,
        isin,
        txn_type,
        quantity:          qty,
        price,
        brokerage,
    })
}

// ─── Summary row parser ───────────────────────────────────────────────────────

fn parse_summary_row(cells: &CellMap, row: usize, date_str: &str) -> Option<SummaryCharge> {
    let stt_paise   = parse_amount_paise_flexible(&pdf_utils::cell_text(cells, row, SUM_COL_STT)).unwrap_or(0);
    let trans_paise = parse_amount_paise_flexible(&pdf_utils::cell_text(cells, row, SUM_COL_TRANS)).unwrap_or(0);
    let stamp_paise = parse_amount_paise_flexible(&pdf_utils::cell_text(cells, row, SUM_COL_STAMP)).unwrap_or(0);

    // Net amount text may overflow into adjacent columns
    let net_text: String = (SUM_COL_NET..SUM_COL_NET + 10)
        .filter_map(|c| {
            let t = pdf_utils::cell_text(cells, row, c);
            if t.is_empty() { None } else { Some(t) }
        })
        .collect::<Vec<_>>()
        .join(" ");
    let net_paise = extract_net_amount_paise(&net_text);

    if stt_paise == 0 && trans_paise == 0 && stamp_paise == 0 && net_paise == 0 {
        return None;
    }

    Some(SummaryCharge {
        trade_date:          parse_date_dmy(date_str.trim()),
        stt_paise,
        trans_charges_paise: trans_paise,
        stamp_charges_paise: stamp_paise,
        total_payable_paise: net_paise,
    })
}

// ─── Amount / date helpers ────────────────────────────────────────────────────

fn parse_amount_paise(s: &str) -> Option<i64> {
    let clean = s.trim().replace(',', "");
    let check = clean.trim_start_matches('-');
    if !amount_re().is_match(check) { return None; }
    let f: f64 = clean.parse().ok()?;
    Some((f * 100.0).round() as i64)
}

fn parse_amount_paise_flexible(s: &str) -> Option<i64> {
    let clean = s.trim().replace(',', "");
    if clean.is_empty() { return None; }
    if let Some(p) = parse_amount_paise(&clean) { return Some(p); }
    let check = clean.trim_start_matches('-');
    if check.chars().all(|c| c.is_ascii_digit()) && !check.is_empty() {
        let n: i64 = clean.parse().ok()?;
        return Some(n * 100);
    }
    None
}

fn extract_net_amount_paise(text: &str) -> i64 {
    if let Some(idx) = text.rfind("Rs.") {
        let after = text[idx + 3..].trim();
        let num_str = after.split_whitespace().next().unwrap_or("").replace(',', "");
        if let Ok(f) = num_str.parse::<f64>() {
            return (f * 100.0).round() as i64;
        }
    }
    0
}

fn parse_date_dmy(s: &str) -> String {
    let p: Vec<&str> = s.split('-').collect();
    if p.len() == 3 { format!("{}-{}-{}", p[2], p[1], p[0]) } else { s.to_string() }
}

// ─── Import command ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn import_icici_equity_trades(
    app:              tauri::AppHandle,
    account_id:       i64,
    transactions:     Vec<IciciEquityTrade>,
    contract_charges: Vec<ContractNoteCharges>,
    file_paths:       Option<Vec<String>>,
) -> Result<IciciEquityImportResult, String> {
    let conn = db::acquire()?;

    let charges_map: HashMap<String, &ContractNoteCharges> =
        contract_charges.iter().map(|c| (c.cn_no.clone(), c)).collect();

    let mut cn_order:  Vec<String>                         = Vec::new();
    let mut cn_trades: HashMap<String, Vec<&IciciEquityTrade>> = HashMap::new();
    for trade in &transactions {
        if !cn_trades.contains_key(&trade.cn_no) {
            cn_order.push(trade.cn_no.clone());
            cn_trades.insert(trade.cn_no.clone(), Vec::new());
        }
        cn_trades.get_mut(&trade.cn_no).unwrap().push(trade);
    }


    let mut imported = 0usize; let mut skipped = 0usize;
    let mut cns_imported = 0usize; let mut cns_skipped = 0usize;
    let mut auto_created = 0usize;
    let mut skipped_details: Vec<SkippedDetail> = Vec::new();
    let mut successful_batches = Vec::new();

    for cn_no in &cn_order {
        let trades = cn_trades.get(cn_no).unwrap();

        let existing_batch_id: Option<i64> = conn.query_row(
            "SELECT batch_id FROM import_batches
             WHERE account_id=?1 AND source_type='ICICI_TRX_EQUITY' AND ref_no=?2",
            rusqlite::params![account_id, cn_no],
            |r| r.get(0),
        ).ok();

        let mut newly_created = false;
        let batch_id = if let Some(id) = existing_batch_id {
            id
        } else {
            newly_created = true;
            let charges   = charges_map.get(cn_no.as_str());
            let trade_date = trades.first().map(|t| t.trade_date.as_str()).unwrap_or("");

            conn.execute(
                "INSERT INTO import_batches
                    (account_id, source_type, file_name, ref_no, broker, batch_trade_date,
                     stt_paise, stamp_charges_paise, gst_paise, trans_charges_paise,
                     other_charges_paise, total_payable_paise)
                 VALUES (?1,'ICICI_TRX_EQUITY',NULL,?2,'ICICI Securities',?3,?4,?5,?6,?7,?8,?9)",
                rusqlite::params![
                    account_id, cn_no, trade_date,
                    charges.map(|c| c.stt_paise).unwrap_or(0),
                    charges.map(|c| c.stamp_charges_paise).unwrap_or(0),
                    charges.map(|c| c.gst_paise).unwrap_or(0),
                    charges.map(|c| c.trans_charges_paise).unwrap_or(0),
                    charges.map(|c| c.other_charges_paise).unwrap_or(0),
                    charges.map(|c| c.total_payable_paise).unwrap_or(0),
                ],
            ).map_err(|e| e.to_string())?;
            conn.last_insert_rowid()
        };

        let mut trades_inserted = 0;

        for trade in trades {
            let (instrument_id, pending_instrument_id) = match common::resolve_equity(
                &conn,
                &trade.security_name,
                Some(&trade.isin),
                None,
                None,
                Some(&trade.exchange),
                &mut auto_created,
            ) {
                Some(pair) => pair,
                None => {
                    skipped += 1;
                    skipped_details.push(SkippedDetail {
                        trade_date: trade.trade_date.clone(), security_name: trade.security_name.clone(),
                        txn_type: trade.txn_type.clone(), quantity: trade.quantity, price: trade.price,
                        reason: "Could not stage instrument".to_string(),
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

            let rows = conn.execute(
                "INSERT INTO transactions
                    (account_id, instrument_id, pending_instrument_id, txn_type, trade_segment,
                     trade_date, txn_time, quantity, price_paise, brokerage_paise, stt_paise,
                     other_charges_paise, total_value_paise, notes, broker_ref, batch_id)
                 VALUES (?1,?2,?3,?4,'DELIVERY',?5,?6,?7,?8,?9,0,0,?10,NULL,?11,?12)
                 ON CONFLICT(account_id, broker_ref) WHERE broker_ref IS NOT NULL DO UPDATE SET
                     instrument_id = excluded.instrument_id,
                     pending_instrument_id = excluded.pending_instrument_id,
                     txn_type = excluded.txn_type,
                     trade_date = excluded.trade_date,
                     txn_time = excluded.txn_time,
                     quantity = excluded.quantity,
                     price_paise = excluded.price_paise,
                     brokerage_paise = excluded.brokerage_paise,
                     total_value_paise = excluded.total_value_paise,
                     batch_id = excluded.batch_id
                 WHERE transactions.instrument_id IS NOT excluded.instrument_id
                    OR transactions.pending_instrument_id IS NOT excluded.pending_instrument_id
                    OR transactions.txn_type IS NOT excluded.txn_type
                    OR transactions.trade_date IS NOT excluded.trade_date
                    OR transactions.txn_time IS NOT excluded.txn_time
                    OR transactions.quantity IS NOT excluded.quantity
                    OR transactions.price_paise IS NOT excluded.price_paise
                    OR transactions.brokerage_paise IS NOT excluded.brokerage_paise
                    OR transactions.total_value_paise IS NOT excluded.total_value_paise",
                rusqlite::params![
                    account_id, instrument_id, pending_instrument_id, trade.txn_type,
                    trade.trade_date, trade.trade_time,
                    trade.quantity, price_paise, brokerage_paise,
                    total_value, trade.broker_ref, batch_id,
                ],
            ).map_err(|e| e.to_string())?;

            if rows > 0 {
                imported += 1;
                trades_inserted += 1;
            } else {
                skipped += 1;
                skipped_details.push(SkippedDetail {
                    trade_date: trade.trade_date.clone(), security_name: trade.security_name.clone(),
                    txn_type: trade.txn_type.clone(), quantity: trade.quantity, price: trade.price,
                    reason: "Duplicate broker_ref".to_string(),
                });
            }
        }

        if trades_inserted == 0 {
            if newly_created {
                let _ = conn.execute("DELETE FROM import_batches WHERE batch_id=?1", [batch_id]);
            } else {
                cns_skipped += 1;
            }
        } else {
            if newly_created {
                cns_imported += 1;

                // Populate charges table from parsed contract note data.
                // Brokerage is summed across all trades in this CN.
                let trade_date = trades.first().map(|t| t.trade_date.as_str()).unwrap_or("");
                let total_brokerage_paise: i64 = trades.iter()
                    .map(|t| (t.brokerage * 100.0).round() as i64)
                    .sum();
                let cn_charges = charges_map.get(cn_no.as_str());

                let charge_rows: &[(&str, i64)] = &[
                    ("BROKERAGE",  total_brokerage_paise),
                    ("STT",        cn_charges.map(|c| c.stt_paise).unwrap_or(0)),
                    ("EXCHANGE",   cn_charges.map(|c| c.trans_charges_paise).unwrap_or(0)),
                    ("STAMP_DUTY", cn_charges.map(|c| c.stamp_charges_paise).unwrap_or(0)),
                    ("GST",        cn_charges.map(|c| c.gst_paise).unwrap_or(0)),
                ];
                for &(charge_type, amount_paise) in charge_rows {
                    if amount_paise <= 0 { continue; }
                    let _ = conn.execute(
                        "INSERT INTO charges
                            (account_id, start_date, end_date, charge_type,
                             amount_paise, source, import_batch_id)
                         VALUES (?1, ?2, ?3, ?4, ?5, 'IMPORT', ?6)",
                        rusqlite::params![
                            account_id, trade_date, trade_date,
                            charge_type, amount_paise, batch_id,
                        ],
                    );
                }
            }
            successful_batches.push(batch_id);
        }
    }

    if imported > 0 {
        if let Some(paths) = file_paths {
            if let Some(name_json) = common::copy_statements(&app, paths) {
                common::update_batch_file_names(&conn, &successful_batches, &name_json);
            }
        }
    }

    drop(conn);
    flag_oversells(account_id)?;

    Ok(IciciEquityImportResult {
        imported, skipped,
        contract_notes_imported: cns_imported,
        contract_notes_skipped:  cns_skipped,
        auto_created_instruments: auto_created,
        skipped_details,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const OCT22_PDF: &str =
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_28-10-2022_1068522.PDF";

    const APR22_PDF: &str =
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_07-04-2022_1098746.PDF";

    const ICICI_PDFS: &[&str] = &[
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_28-10-2022_1068522.PDF",
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_21-04-2024_1646876.PDF",
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_10-04-2025_1498546.PDF",
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_09-10-2021_962695.PDF",
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_07-04-2024_61770.PDF",
        APR22_PDF,
    ];

    /// Full parse smoke test: all available ICICI PDFs must yield trades.
    #[test]
    fn test_parse_all() {
        for path in ICICI_PDFS {
            if !std::path::Path::new(path).exists() { continue; }
            let result = parse_icici_equity_pdf(path.to_string(), None).expect("parse failed");
            let fname = std::path::Path::new(path).file_name().unwrap().to_string_lossy();
            println!("\n=== {fname} ===");
            println!("  trades={} CNs={} client={:?} range={:?}",
                result.total_rows, result.contract_charges.len(),
                result.client_code, result.date_range);
            for t in &result.transactions {
                println!("  {} {} {} qty={} @ ₹{} | {} | {:?}",
                    t.trade_date, t.exchange, t.txn_type, t.quantity, t.price,
                    t.cn_no, t.broker_ref);
            }
            for cn in &result.contract_charges {
                println!("  CHARGES {} | stt={} trans={} stamp={} total={}",
                    cn.cn_no, cn.stt_paise, cn.trans_charges_paise,
                    cn.stamp_charges_paise, cn.total_payable_paise);
            }
            assert!(result.total_rows > 0, "expected trades in {path}");
        }
    }

    /// Oct-2022 known file: must parse exactly 4 trades with correct ISIN / charges.
    #[test]
    fn test_oct22_detail() {
        if !std::path::Path::new(OCT22_PDF).exists() { return; }
        let result = parse_icici_equity_pdf(OCT22_PDF.to_string(), None).expect("parse failed");
        println!("\nOct-2022 — {} trades, {} CNs", result.total_rows, result.contract_charges.len());
        for t in &result.transactions {
            println!("  {} {} {} qty={} @ ₹{} isin={} cn={} ref={:?} time={:?}",
                t.trade_date, t.exchange, t.txn_type, t.quantity, t.price,
                t.isin, t.cn_no, t.broker_ref, t.trade_time);
        }
        for cn in &result.contract_charges {
            println!("  CHARGES {} | stt={} trans={} stamp={} total={}",
                cn.cn_no, cn.stt_paise, cn.trans_charges_paise,
                cn.stamp_charges_paise, cn.total_payable_paise);
        }
        assert_eq!(result.total_rows, 4, "expected 4 trades");
        assert!(result.transactions.iter().all(|t| !t.isin.is_empty()), "all trades need ISIN");
        assert!(result.transactions.iter().all(|t| !t.cn_no.contains("FALLBACK")), "no fallback CNs");
    }

    /// Every trade in every file must carry a non-empty ISIN and a CN number.
    #[test]
    fn test_isin_and_cn_present() {
        for path in ICICI_PDFS {
            if !std::path::Path::new(path).exists() { continue; }
            let result = parse_icici_equity_pdf(path.to_string(), None).expect("parse failed");
            let no_isin: Vec<_> = result.transactions.iter().filter(|t| t.isin.is_empty()).collect();
            let fallback: Vec<_> = result.transactions.iter().filter(|t| t.cn_no.contains("FALLBACK")).collect();
            println!("{}: trades={} no_isin={} fallback_cn={}", path,
                result.total_rows, no_isin.len(), fallback.len());
            assert!(no_isin.is_empty(), "missing ISIN in {path}");
            assert!(fallback.is_empty(), "fallback CN in {path}");
        }
    }

    // ── Grid / debug tests (kept for visualisation and diagnosis) ────────────

    /// Dump every vertical line segment from both 2021 and 2022 PDFs with
    /// graphics-state properties: width, stroke colour, stroke alpha.
    #[test]
    fn test_dump_vline_properties() {
        use lopdf::content::Content;
        use lopdf::Object;

        fn obj_f32(o: &Object) -> Option<f32> {
            match o {
                Object::Integer(i) => Some(*i as f32),
                Object::Real(r)    => Some(*r as f32),
                _ => None,
            }
        }

        // Minimal 2-D affine state
        type Mat = [f32; 6];
        const ID: Mat = [1.0,0.0,0.0,1.0,0.0,0.0];
        fn mul(m: Mat, n: Mat) -> Mat {
            let [a1,b1,c1,d1,e1,f1]=m; let [a2,b2,c2,d2,e2,f2]=n;
            [a1*a2+b1*c2,a1*b2+b1*d2,c1*a2+d1*c2,c1*b2+d1*d2,
             e1*a2+f1*c2+e2,e1*b2+f1*d2+f2]
        }
        fn pt(m: Mat, x: f32, y: f32) -> (f32,f32) {
            let [a,b,c,d,e,f]=m; (a*x+c*y+e, b*x+d*y+f)
        }

        #[derive(Clone)]
        struct GS {
            ctm:          Mat,
            line_width:   f32,
            stroke_gray:  f32,   // 0=black 1=white; -1=rgb/cmyk
            stroke_rgb:   Option<(f32,f32,f32)>,
            stroke_alpha: f32,   // 1.0 = fully opaque
        }
        impl Default for GS {
            fn default() -> Self {
                GS { ctm: ID, line_width: 1.0, stroke_gray: 0.0,
                     stroke_rgb: None, stroke_alpha: 1.0 }
            }
        }

        let files: &[(&str, &str)] = &[
            (r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_28-10-2022_1068522.PDF", "2022"),
            (r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_09-10-2021_962695.PDF", "2021"),
        ];

        for &(path, label) in files {
            if !std::path::Path::new(path).exists() { continue; }
            let doc = lopdf::Document::load(path).expect("load");
            let page_num = *doc.get_pages().keys().next().unwrap();

            let page_id = *doc.get_pages().get(&page_num).unwrap();
            // ExtGState lookup skipped — inline w/G/RG operators are sufficient
            let ext_gs: std::collections::HashMap<String,(f32,f32)> = std::collections::HashMap::new();

            let bytes = doc.get_page_content(page_id).expect("content");
            let content = Content::decode(&bytes).expect("decode");

            let mut stack: Vec<GS> = vec![GS::default()];
            let mut current: Option<(f32,f32)> = None;
            let mut pending: Vec<(f32,f32,f32,f32)> = Vec::new();

            #[derive(Debug)]
            struct VLine { x: f32, y1: f32, y2: f32, len: f32, width: f32, gray: f32, rgb: Option<(f32,f32,f32)>, alpha: f32 }
            let mut vlines: Vec<VLine> = Vec::new();

            macro_rules! gs { () => { stack.last_mut().unwrap() } }

            for op in &content.operations {
                let ctm = gs!().ctm;
                match op.operator.as_str() {
                    "q"  => { stack.push(stack.last().unwrap().clone()); }
                    "Q"  => { if stack.len()>1 { stack.pop(); } }
                    "cm" if op.operands.len()==6 => {
                        if let (Some(a),Some(b),Some(c),Some(d),Some(e),Some(f))=(
                            obj_f32(&op.operands[0]),obj_f32(&op.operands[1]),
                            obj_f32(&op.operands[2]),obj_f32(&op.operands[3]),
                            obj_f32(&op.operands[4]),obj_f32(&op.operands[5]))
                        { gs!().ctm = mul([a,b,c,d,e,f], ctm); }
                    }
                    "w" if op.operands.len()==1 => {
                        if let Some(w) = obj_f32(&op.operands[0]) { gs!().line_width = w; }
                    }
                    "G" if op.operands.len()==1 => {
                        if let Some(g) = obj_f32(&op.operands[0]) {
                            gs!().stroke_gray = g; gs!().stroke_rgb = None;
                        }
                    }
                    "RG" if op.operands.len()==3 => {
                        if let (Some(r),Some(g),Some(b))=(
                            obj_f32(&op.operands[0]),obj_f32(&op.operands[1]),obj_f32(&op.operands[2]))
                        { gs!().stroke_rgb = Some((r,g,b)); gs!().stroke_gray = -1.0; }
                    }
                    "gs" if op.operands.len()==1 => {
                        if let lopdf::Object::Name(name) = &op.operands[0] {
                            let key = String::from_utf8_lossy(name).to_string();
                            if let Some(&(ca, lw)) = ext_gs.get(&key) {
                                gs!().stroke_alpha = ca;
                                if lw >= 0.0 { gs!().line_width = lw; }
                            }
                        }
                    }
                    "m" if op.operands.len()==2 => {
                        if let (Some(x),Some(y))=(obj_f32(&op.operands[0]),obj_f32(&op.operands[1])) {
                            current = Some(pt(ctm,x,y));
                        }
                    }
                    "l" if op.operands.len()==2 => {
                        if let (Some(x),Some(y))=(obj_f32(&op.operands[0]),obj_f32(&op.operands[1])) {
                            let p = pt(ctm,x,y);
                            if let Some(c) = current { pending.push((c.0,c.1,p.0,p.1)); }
                            current = Some(p);
                        }
                    }
                    "re" if op.operands.len()==4 => {
                        if let (Some(x),Some(y),Some(w),Some(h))=(
                            obj_f32(&op.operands[0]),obj_f32(&op.operands[1]),
                            obj_f32(&op.operands[2]),obj_f32(&op.operands[3]))
                        {
                            let (x0,y0)=pt(ctm,x,y); let (x1,y1)=pt(ctm,x+w,y);
                            let (x2,y2)=pt(ctm,x+w,y+h); let (x3,y3)=pt(ctm,x,y+h);
                            pending.extend_from_slice(&[(x0,y0,x1,y1),(x1,y1,x2,y2),(x2,y2,x3,y3),(x3,y3,x0,y0)]);
                            current = Some((x0,y0));
                        }
                    }
                    "S"|"s"|"B"|"B*"|"b"|"b*" => {
                        let g = gs!().clone();
                        for (x1,y1,x2,y2) in pending.drain(..) {
                            // vertical: x delta < 1, y delta >= MIN_V_LEN
                            if (x2-x1).abs() < 1.0 && (y2-y1).abs() >= MIN_V_LEN {
                                vlines.push(VLine {
                                    x: x1, y1: y1.min(y2), y2: y1.max(y2),
                                    len: (y2-y1).abs(),
                                    width: g.line_width,
                                    gray: g.stroke_gray,
                                    rgb: g.stroke_rgb,
                                    alpha: g.stroke_alpha,
                                });
                            }
                        }
                        current = None;
                    }
                    "f"|"F"|"f*"|"n" => { pending.clear(); current = None; }
                    _ => {}
                }
            }

            // Cluster X to find unique column positions
            let xs: Vec<f32> = vlines.iter().map(|l| l.x).collect();
            let col_xs = pdf_utils::cluster_coords(xs.clone(), CLUSTER_GAP);

            println!("\n\n══ {label} — {} vertical lines, {} unique X positions ══", vlines.len(), col_xs.len());
            println!("{:<6} {:<6} {:<5} {:<7} {:<7} {:<7} {:<7} {:<7}  colour",
                     "x", "col#", "cnt", "length", "y_min", "y_max", "width", "alpha");
            println!("{}", "─".repeat(80));

            for &cx in &col_xs {
                let group: Vec<&VLine> = vlines.iter().filter(|l| (l.x - cx).abs() <= CLUSTER_GAP).collect();
                if group.is_empty() { continue; }
                let sample = group[0];
                let colour = match sample.rgb {
                    Some((r,g,b)) => format!("rgb({:.2},{:.2},{:.2})", r,g,b),
                    None          => format!("gray({:.2})", sample.gray),
                };
                let col_idx = col_xs.iter().position(|&c| (c-cx).abs()<0.1).unwrap_or(99);
                let cnt = group.len();
                println!("{:<6.0} [{:<3}] {:<5} {:<7.1} {:<7.1} {:<7.1} {:<7.2} {:<7.2}  {}",
                         cx, col_idx, cnt, sample.len, sample.y1, sample.y2,
                         sample.width, sample.alpha, colour);
            }
        }
    }

    /// Dump all line segments from page 1 of the Oct-2022 PDF.
    #[test]
    fn test_dump_lines_oct22() {
        if !std::path::Path::new(OCT22_PDF).exists() { return; }
        let doc = lopdf::Document::load(OCT22_PDF).expect("failed to load PDF");
        let pages = doc.get_pages();
        println!("PDF has {} page(s)", pages.len());
        for (&page_num, _) in &pages {
            let lines = pdf_utils::extract_page_lines(&doc, page_num);
            let h: Vec<_> = lines.iter().filter(|l| l.is_horizontal(1.0)).collect();
            let v: Vec<_> = lines.iter().filter(|l| l.is_vertical(1.0)).collect();
            println!("\nPage {}: total={} H={} V={} diag={}",
                page_num, lines.len(), h.len(), v.len(), lines.len()-h.len()-v.len());
            let (row_ys, col_xs) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);
            println!("  Row Ys ({}): {:?}", row_ys.len(), row_ys);
            println!("  Col Xs ({}): {:?}", col_xs.len(), col_xs);
        }
    }

    /// Write HTML visualisation of grid + text spans.
    #[test]
    fn test_html_grid_viz() {
        if !std::path::Path::new(OCT22_PDF).exists() { return; }
        let doc = lopdf::Document::load(OCT22_PDF).expect("failed to load PDF");
        let pages = doc.get_pages();
        let page_num = *pages.keys().next().expect("no pages");

        let lines = pdf_utils::extract_page_lines(&doc, page_num);
        let (row_ys, col_xs) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);

        let all_spans = pdf_utils::extract_all_page_spans_cfg(OCT22_PDF, X_GAP, CHAR_Y_TOL)
            .expect("span extraction failed");
        let page_spans = all_spans.into_iter().next().unwrap_or_default();

        let all_xs: Vec<f32> = lines.iter().flat_map(|l| [l.x1, l.x2]).collect();
        let all_ys: Vec<f32> = lines.iter().flat_map(|l| [l.y1, l.y2]).collect();
        let min_x = all_xs.iter().cloned().fold(f32::INFINITY,  f32::min);
        let max_x = all_xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
        let min_y = all_ys.iter().cloned().fold(f32::INFINITY,  f32::min);
        let max_y = all_ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

        let pad = 20.0f32; let scale = 0.9f32;
        let w = (max_x - min_x) * scale + pad * 2.0;
        let h = (max_y - min_y) * scale + pad * 2.0;
        let tx = |x: f32| (x - min_x) * scale + pad;
        let ty = |y: f32| (max_y - y) * scale + pad;

        let cells: Vec<(usize, usize, String, f32, f32, f32)> = page_spans.iter().map(|span| {
            let col = col_xs.partition_point(|&cx| cx <= span.x + COL_SNAP).saturating_sub(1);
            let row = row_ys.partition_point(|&ry| ry < span.y);
            (row, col, span.text.clone(), span.x, span.y, span.right)
        }).collect();

        let n_rows = row_ys.len() + 1;
        let row_colours: Vec<&str> = vec![
            "#fff7cc","#d4edda","#cce5ff","#f8d7da","#e2d9f3",
            "#fde2c8","#d1ecf1","#dff0d8","#fcf8e3","#e8daef",
            "#d5f5e3","#fef9e7","#ebdef0","#d6eaf8",
        ];
        let row_colour = |r: usize| row_colours[r % row_colours.len()];

        let mut svg = String::new();
        svg.push_str(&format!(r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" style="font-family:monospace;font-size:6px;background:#f5f5f5">"##));
        svg.push_str(&format!(r##"<rect x="{}" y="{}" width="{}" height="{}" fill="white" stroke="#999" stroke-width="0.5"/>"##,
            tx(min_x), ty(max_y), (max_x-min_x)*scale, (max_y-min_y)*scale));

        for i in 0..row_ys.len().saturating_sub(1) {
            let sy = ty(row_ys[i+1]); let sh = (row_ys[i+1]-row_ys[i])*scale;
            svg.push_str(&format!(r##"<rect x="{}" y="{sy:.1}" width="{}" height="{sh:.1}" fill="{}" opacity="0.35"/>"##,
                tx(min_x), (max_x-min_x)*scale, row_colour(i)));
        }
        for &cx in &col_xs {
            svg.push_str(&format!(r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="#bbb" stroke-width="0.4" stroke-dasharray="2,3"/>"##,
                tx(cx), ty(max_y), tx(cx), ty(min_y)));
        }
        for l in &lines {
            let (sx,sy,ex,ey) = (tx(l.x1),ty(l.y1),tx(l.x2),ty(l.y2));
            let (colour,width) = if l.is_horizontal(1.0) { ("#2c7be5",1.2) }
                                 else if l.is_vertical(1.0) { ("#e63946",1.2) }
                                 else { ("#888",0.8) };
            svg.push_str(&format!(r##"<line x1="{sx:.1}" y1="{sy:.1}" x2="{ex:.1}" y2="{ey:.1}" stroke="{colour}" stroke-width="{width}"/>"##));
        }
        for (row, _, txt, x, y, right) in &cells {
            let sx = tx(*x); let ex = tx(right.max(*x+4.0)); let cy = ty(*y);
            let box_h = 7.0f32;
            let txt_e = txt.replace('&',"&amp;").replace('<',"&lt;").replace('>',"&gt;");
            svg.push_str(&format!(r##"<rect x="{sx:.1}" y="{:.1}" width="{:.1}" height="{box_h:.1}" fill="{}" stroke="#555" stroke-width="0.3" opacity="0.85"/>"##,
                cy-box_h+1.5, (ex-sx).max(3.0), row_colour(*row)));
            svg.push_str(&format!(r##"<text x="{sx:.1}" y="{cy:.1}" fill="#111" font-size="5.5">{txt_e}</text>"##));
        }
        svg.push_str("</svg>");

        let mut legend_rows = String::new();
        for i in 0..n_rows.min(row_colours.len()) {
            let label = if i < row_ys.len() { format!("row {i} (y≤{:.0})", row_ys[i]) }
                        else { format!("row {i} (above grid)") };
            legend_rows.push_str(&format!(r##"<li><span style="background:{};padding:2px 8px;border:1px solid #aaa">&nbsp;</span> {label}</li>"##, row_colour(i)));
        }

        let html = format!(r##"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ICICI Grid</title>
<style>body{{font-family:sans-serif;margin:20px;background:#eee}}.legend{{display:flex;gap:12px;flex-wrap:wrap;margin-bottom:12px;font-size:.75rem}}.key-box{{background:#fff;padding:8px 12px;border:1px solid #ccc;border-radius:4px}}.svg-wrap{{overflow:auto;background:#fff;border:1px solid #ccc;border-radius:4px;padding:4px}}</style>
</head><body>
<h1 style="font-size:1.1rem">ICICI Securities — Border-based Grid · Page 1 · Oct-2022</h1>
<div class="legend">
  <div class="key-box"><strong>Lines</strong><br><span style="color:#2c7be5">━━</span> Horizontal&nbsp;&nbsp;<span style="color:#e63946">━━</span> Vertical&nbsp;&nbsp;<span style="color:#bbb">- -</span> Col boundary</div>
  <div class="key-box"><strong>Grid rows ({n_rows})</strong><ul>{legend_rows}</ul></div>
</div>
<div class="svg-wrap">{svg}</div>
</body></html>"##, n_rows=n_rows);

        let out_path = r"C:\Users\SUMIT\AppData\Local\Temp\icici2_grid.html";
        std::fs::write(out_path, &html).expect("failed to write HTML");
        println!("Wrote {out_path}");
    }

    /// Generate HTML grid visualisation for the 2021 and 2022 PDFs.
    #[test]
    fn test_html_grid_compare() {
        let files: &[(&str, &str)] = &[
            (r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_28-10-2022_1068522.PDF", "2022"),
            (r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_09-10-2021_962695.PDF", "2021"),
        ];

        let row_colours: &[&str] = &[
            "#fff7cc","#d4edda","#cce5ff","#f8d7da","#e2d9f3",
            "#fde2c8","#d1ecf1","#dff0d8","#fcf8e3","#e8daef",
            "#d5f5e3","#fef9e7","#ebdef0","#d6eaf8",
        ];

        // Column indices the parser reads — highlighted in red
        let named_cols: &[(usize, &str)] = &[
            (0,  "CN"),
            (3,  "EXCHANGE"),
            (8,  "TRADE_NO"),
            (10, "TRADE_DT"),
            (13, "ISIN"),
            (14, "SECURITY"),
            (16, "B/S"),
            (18, "QTY"),
            (19, "PRICE"),
            (21, "BROKERAGE"),
        ];

        for &(path, label) in files {
            if !std::path::Path::new(path).exists() {
                println!("{label}: file not found, skipping");
                continue;
            }
            let doc = lopdf::Document::load(path).expect("load failed");
            let page_num = *doc.get_pages().keys().next().unwrap();

            let lines = pdf_utils::extract_page_lines(&doc, page_num);
            let (row_ys, col_xs) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);

            let all_spans = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], X_GAP, CHAR_Y_TOL)
                .expect("span extract failed");
            let page_spans = all_spans.into_iter().next().unwrap_or_default();

            let all_xs: Vec<f32> = lines.iter().flat_map(|l| [l.x1, l.x2]).collect();
            let all_ys: Vec<f32> = lines.iter().flat_map(|l| [l.y1, l.y2]).collect();
            let min_x = all_xs.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_x = all_xs.iter().cloned().fold(f32::NEG_INFINITY, f32::max);
            let min_y = all_ys.iter().cloned().fold(f32::INFINITY, f32::min);
            let max_y = all_ys.iter().cloned().fold(f32::NEG_INFINITY, f32::max);

            let pad = 20.0f32; let scale = 0.9f32;
            let w = (max_x - min_x) * scale + pad * 2.0;
            let h = (max_y - min_y) * scale + pad * 2.0 + 20.0;
            let tx = |x: f32| (x - min_x) * scale + pad;
            let ty = |y: f32| (max_y - y) * scale + pad;

            let span_cells: Vec<(usize, usize, &str, f32, f32, f32)> = page_spans.iter().map(|s| {
                let col = col_xs.partition_point(|&cx| cx <= s.x + COL_SNAP).saturating_sub(1);
                let row = row_ys.partition_point(|&ry| ry < s.y);
                (row, col, s.text.as_str(), s.x, s.y, s.right)
            }).collect();

            let named_col_set: std::collections::HashSet<usize> =
                named_cols.iter().map(|&(c, _)| c).collect();

            let mut svg = String::new();
            svg.push_str(&format!(
                r##"<svg xmlns="http://www.w3.org/2000/svg" width="{w:.0}" height="{h:.0}" style="font-family:monospace;font-size:6px;background:#f5f5f5">"##
            ));
            svg.push_str(&format!(
                r##"<rect x="{}" y="{}" width="{}" height="{}" fill="white" stroke="#999" stroke-width="0.5"/>"##,
                tx(min_x), ty(max_y), (max_x - min_x) * scale, (max_y - min_y) * scale
            ));

            for i in 0..row_ys.len().saturating_sub(1) {
                let sy = ty(row_ys[i + 1]);
                let sh = (row_ys[i + 1] - row_ys[i]) * scale;
                svg.push_str(&format!(
                    r##"<rect x="{}" y="{sy:.1}" width="{}" height="{sh:.1}" fill="{}" opacity="0.3"/>"##,
                    tx(min_x), (max_x - min_x) * scale, row_colours[i % row_colours.len()]
                ));
            }

            for (ci, &cx) in col_xs.iter().enumerate() {
                let is_named = named_col_set.contains(&ci);
                let col_c  = if is_named { "#e63946" } else { "#ccc" };
                let col_w  = if is_named { "1.2" } else { "0.4" };
                let dash   = if is_named { "" } else { r##" stroke-dasharray="2,3""## };
                svg.push_str(&format!(
                    r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{col_c}" stroke-width="{col_w}"{dash}/>"##,
                    tx(cx), ty(max_y), tx(cx), ty(min_y)
                ));
                let lbl_c = if is_named { "#c00" } else { "#aaa" };
                let lbl_w = if is_named { "bold" } else { "normal" };
                svg.push_str(&format!(
                    r##"<text x="{:.1}" y="{:.1}" fill="{lbl_c}" font-size="5" font-weight="{lbl_w}">{ci}</text>"##,
                    tx(cx) + 1.0, ty(max_y) - 2.0
                ));
            }

            for &(ci, name) in named_cols {
                if ci < col_xs.len() {
                    svg.push_str(&format!(
                        r##"<text x="{:.1}" y="{:.1}" fill="#c00" font-size="5" font-weight="bold">{name}</text>"##,
                        tx(col_xs[ci]) + 1.0, ty(min_y) + 10.0
                    ));
                }
            }

            for l in &lines {
                let (lc, lw) = if l.is_horizontal(1.0) { ("#2c7be5", "1.0") }
                               else if l.is_vertical(1.0) { ("#bbb", "0.5") }
                               else { ("#888", "0.5") };
                svg.push_str(&format!(
                    r##"<line x1="{:.1}" y1="{:.1}" x2="{:.1}" y2="{:.1}" stroke="{lc}" stroke-width="{lw}"/>"##,
                    tx(l.x1), ty(l.y1), tx(l.x2), ty(l.y2)
                ));
            }

            for &(row, _, txt, x, y, right) in &span_cells {
                let sx = tx(x); let ex = tx(right.max(x + 4.0)); let cy = ty(y);
                let bh = 7.0f32;
                let te = txt.replace('&', "&amp;").replace('<', "&lt;").replace('>', "&gt;");
                let fill = row_colours[row % row_colours.len()];
                svg.push_str(&format!(
                    r##"<rect x="{sx:.1}" y="{:.1}" width="{:.1}" height="{bh:.1}" fill="{fill}" stroke="#555" stroke-width="0.3" opacity="0.85"/>"##,
                    cy - bh + 1.5, (ex - sx).max(3.0)
                ));
                svg.push_str(&format!(
                    r##"<text x="{sx:.1}" y="{cy:.1}" fill="#111" font-size="5.5">{te}</text>"##
                ));
            }
            svg.push_str("</svg>");

            let mut legend = String::new();
            for &(ci, name) in named_cols {
                legend.push_str(&format!(r##"<span class="tag">[{ci}] {name}</span> "##));
            }

            let col_xs_str = format!("{:?}", col_xs.iter().map(|x| *x as i32).collect::<Vec<_>>());
            let n = col_xs.len();

            let html = format!(r##"<!DOCTYPE html>
<html><head><meta charset="utf-8"><title>ICICI Grid — {label}</title>
<style>
  body{{font-family:sans-serif;margin:20px;background:#eee}}
  h1{{font-size:1rem;margin-bottom:6px}}
  .legend{{display:flex;gap:5px;flex-wrap:wrap;margin-bottom:8px;font-size:.75rem;align-items:center}}
  .tag{{background:#fff;border:1px solid #c00;color:#c00;padding:2px 5px;border-radius:3px;font-family:monospace;font-size:.7rem}}
  .info{{font-size:.75rem;color:#555;margin-bottom:6px;font-family:monospace}}
  .svg-wrap{{overflow:auto;background:#fff;border:1px solid #ccc;border-radius:4px;padding:4px}}
</style></head><body>
<h1>ICICI Securities — Column border visualisation &middot; <b>{label}</b> format (page 1)</h1>
<p class="info">col_xs ({n}): {col_xs_str}</p>
<div class="legend"><b>Parser cols (red):</b> {legend}</div>
<p class="info"><span style="color:#2c7be5;font-weight:bold">━━</span> Horizontal PDF line &nbsp;&nbsp;
<span style="color:#c00;font-weight:bold">━━</span> Named column boundary (parser reads) &nbsp;&nbsp;
<span style="color:#ccc">- -</span> Other column boundary</p>
<div class="svg-wrap">{svg}</div>
</body></html>"##);

            let out_path = format!(r"C:\Users\SUMIT\AppData\Local\Temp\icici_grid_{label}.html");
            std::fs::write(&out_path, &html).expect("write failed");
            println!("Wrote {out_path}");
        }
    }

    /// Dump grid structure for the Apr-2022 PDF to diagnose column mapping.
    #[test]
    fn test_dump_grid_apr22() {
        if !std::path::Path::new(APR22_PDF).exists() { return; }
        let doc = lopdf::Document::load(APR22_PDF).expect("failed to load PDF");
        let all_spans = pdf_utils::extract_spans_from_doc_cfg(&doc, &[], X_GAP, CHAR_Y_TOL)
            .expect("span extraction failed");

        let pages: Vec<u32> = doc.get_pages().keys().copied().collect();
        for (page_num, page_spans) in pages.iter().zip(all_spans.iter()) {
            let lines = pdf_utils::extract_page_lines(&doc, *page_num);
            let (row_ys, col_xs) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);
            println!("\n=== Page {} ===", page_num);
            println!("  {} col boundaries: {:?}", col_xs.len(), col_xs);
            println!("  {} row boundaries: {:?}", row_ys.len(), row_ys);

            if col_xs.is_empty() { println!("  NO GRID DETECTED"); continue; }

            let cells = pdf_utils::build_cell_map(page_spans, &row_ys, &col_xs, COL_SNAP);
            let n_rows = row_ys.len() + 1;
            println!("  Cell map ({} rows):", n_rows);
            for row in 0..n_rows {
                let row_data: Vec<String> = (0..col_xs.len())
                    .map(|col| {
                        let t = pdf_utils::cell_text(&cells, row, col);
                        if t.is_empty() { "_".to_string() } else { format!("[{col}]{}", t.replace('\n', "↵")) }
                    })
                    .filter(|s| s != "_")
                    .collect();
                if !row_data.is_empty() {
                    println!("    row {row}: {}", row_data.join("  "));
                }
            }
        }
    }

    /// Dump col_xs and row_ys for every page of all 4 known ICICI PDFs.
    #[test]
    fn test_dump_grid_all_files() {
        let files = &[
            r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_28-10-2022_1068522.PDF",
            r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_21-04-2024_1646876.PDF",
            r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_09-10-2021_962695.PDF",
            r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\TRX-Equity_10-04-2025_1498546.PDF",
        ];
        for path in files {
            if !std::path::Path::new(path).exists() { continue; }
            let fname = std::path::Path::new(path).file_name().unwrap().to_string_lossy();
            let doc = lopdf::Document::load(path).expect("load failed");
            println!("\n\n══════════════════════════════════════════════════════");
            println!("FILE: {fname}");
            println!("══════════════════════════════════════════════════════");
            for (&page_num, _) in &doc.get_pages() {
                let lines = pdf_utils::extract_page_lines(&doc, page_num);
                let h: Vec<_> = lines.iter().filter(|l| l.is_horizontal(1.0)).collect();
                let v: Vec<_> = lines.iter().filter(|l| l.is_vertical(1.0)).collect();
                let (row_ys, col_xs) = pdf_utils::grid_from_lines(&lines, MIN_H_LEN, MIN_V_LEN, CLUSTER_GAP);
                println!("\n  Page {page_num}: H={} V={}", h.len(), v.len());
                println!("  col_xs ({}): {:?}", col_xs.len(), col_xs);
                println!("  row_ys ({}): {:?}", row_ys.len(), row_ys);
            }
        }
    }

    // ── CAMS investigation ────────────────────────────────────────────────────

    const CAMS_PDF: &str =
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\CAMS_Report.pdf";

    /// Investigate whether the CAMS PDF has border lines, and show the
    /// distribution of text-span X-start and right-edge positions.
    /// This determines whether a border-based or right-edge-clustering approach
    /// can replace the current fixed column-boundary constants in cams_cas.rs.
    #[test]
    fn test_cams_structure() {
        if !std::path::Path::new(CAMS_PDF).exists() {
            println!("CAMS PDF not found — skipping");
            return;
        }
        let doc = lopdf::Document::load(CAMS_PDF).expect("failed to load");
        let pages = doc.get_pages();
        println!("CAMS PDF: {} pages", pages.len());

        // Check first 3 pages
        for (&page_num, _) in pages.iter().take(3) {
            let lines = pdf_utils::extract_page_lines(&doc, page_num);
            let h: Vec<_> = lines.iter().filter(|l| l.is_horizontal(1.0)).collect();
            let v: Vec<_> = lines.iter().filter(|l| l.is_vertical(1.0)).collect();
            println!("\nPage {}: total lines={} H={} V={} diag={}",
                page_num, lines.len(), h.len(), v.len(),
                lines.len()-h.len()-v.len());

            let long_h: Vec<_> = h.iter().filter(|l| l.length() > 50.0).collect();
            let long_v: Vec<_> = v.iter().filter(|l| l.length() > 20.0).collect();
            println!("  Long H (>50pt): {}  Long V (>20pt): {}", long_h.len(), long_v.len());
        }

        // Show X-position and right-edge distribution from page 1 spans
        let all_spans = pdf_utils::extract_all_page_spans_cfg(CAMS_PDF, X_GAP, CHAR_Y_TOL)
            .expect("span extraction failed");
        let page1 = all_spans.into_iter().next().unwrap_or_default();

        // Cluster left-edge X positions
        let left_xs: Vec<f32> = page1.iter().map(|s| s.x).collect();
        let left_clusters = pdf_utils::cluster_coords(left_xs, 4.0);
        println!("\nPage 1 left-edge X clusters ({}):", left_clusters.len());
        println!("  {:?}", left_clusters);

        // Cluster right-edge positions of numeric-looking spans
        let numeric_rights: Vec<f32> = page1.iter()
            .filter(|s| {
                let t = s.text.trim().replace(',', "").replace('.', "");
                t.len() >= 3 && t.chars().all(|c| c.is_ascii_digit())
            })
            .map(|s| s.right)
            .collect();
        let right_clusters = pdf_utils::cluster_coords(numeric_rights, 4.0);
        println!("\nPage 1 numeric right-edge clusters ({}):", right_clusters.len());
        println!("  {:?}", right_clusters);

        // Show a sample of rows with their spans
        let rows = pdf_utils::page_spans_to_rows(page1, 5.0);
        println!("\nFirst 20 rows (spans with x and text):");
        for row in rows.iter().take(20) {
            let line: Vec<String> = row.iter()
                .map(|s| format!("[x={:.0}..{:.0}] {:?}", s.x, s.right, s.text))
                .collect();
            println!("  {}", line.join("  "));
        }
    }
}
