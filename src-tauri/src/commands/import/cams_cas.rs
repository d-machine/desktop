//! Parser for CAMS Consolidated Account Statement (CAS) PDF.
//!
//! Structure per fund:
//!   <AMC Name>
//!   <Scheme Code>-<Scheme Name> - ISIN: INF...
//!   Folio No: XXXX
//!   <investor name, nominees — skip>
//!   Opening Unit Balance: X.XXX
//!   DD-Mon-YYYY  <Description>  <Amount>  <Units>  <NAV>  <Balance>
//!   [            *** Stamp Duty ***  X.XX                          ]
//!   [            *** STT Paid ***    X.XX                          ]
//!   Closing Unit Balance: X.XXX  Total Cost Value: X.XX

use crate::{commands::import::pdf_utils, db};
use regex::Regex;
use serde::{Deserialize, Serialize};
use std::sync::OnceLock;

// ─── Regex helpers ────────────────────────────────────────────────────────────

fn date_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Allow zero whitespace between date and rest — pdf_extract sometimes merges them.
    RE.get_or_init(|| Regex::new(r"^(\d{2}-[A-Za-z]{3}-\d{4})\s*(.*)").unwrap())
}
fn isin_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"ISIN\s*:\s*(IN[A-Z0-9]{10})").unwrap())
}
fn bare_isin_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\b(IN[A-Z0-9]{10})\b").unwrap())
}
fn folio_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)folio\s*(?:no\.?|num\.?|number)?\s*:?\s*(\w+(?:[/.-]\w+)*)").unwrap())
}
fn closing_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)closing\s+unit\s+balance\s*:\s*([\d,]+\.\d+)").unwrap())
}
fn opening_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)opening\s+unit\s+balance\s*:\s*([\d,]+\.\d+)").unwrap())
}
fn stamp_duty_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\*+\s*stamp\s+duty\s*\*+.*?([\d,.]+)\s*$").unwrap())
}
fn stt_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(?i)\*+\s*stt\s+paid\s*\*+.*?([\d,.]+)\s*$").unwrap())
}
fn number_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"-?[\d,]+\.\d+").unwrap())
}
fn paren_neg_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    // Matches parenthesised negatives like "(25,974.81)" → converts to "-25974.81"
    RE.get_or_init(|| Regex::new(r"\(([\d,]+\.\d+)\)").unwrap())
}
/// Convert CAMS parenthesised negatives to signed numbers, e.g. "(25,974.81)" → "-25974.81".
fn normalise_signs(s: &str) -> String {
    paren_neg_re().replace_all(s, "-$1").into_owned()
}
fn pan_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"\bPAN\s*:\s*([A-Z]{5}\d{4}[A-Z])\b").unwrap())
}
fn period_re() -> &'static Regex {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"(\d{2}-[A-Za-z]{3}-\d{4})\s+[Tt]o\s+(\d{2}-[A-Za-z]{3}-\d{4})").unwrap())
}

// ─── Types ────────────────────────────────────────────────────────────────────

#[derive(Debug, Serialize, Deserialize, Clone)]
pub struct CasTransaction {
    pub date:          String,
    pub description:   String,
    pub txn_type:      String,
    pub amount_rs:     f64,
    pub units:         f64,
    pub nav_rs:        f64,
    pub unit_balance:  f64,
    pub stamp_duty_rs: f64,
    pub stt_rs:        f64,
}

#[derive(Debug, Serialize, Deserialize)]
pub struct CasFundPreview {
    pub amc:             String,
    pub scheme:          String,
    pub isin:            String,
    pub folio:           String,
    pub pan:             String,
    pub opening_balance: f64,
    pub closing_balance: f64,
    pub transactions:    Vec<CasTransaction>,
}

#[derive(Debug, Serialize)]
pub struct CasPreview {
    pub investor_name:      String,
    pub pan:                String,
    pub period_from:        String,
    pub period_to:          String,
    pub funds:              Vec<CasFundPreview>,
    pub total_transactions: usize,
}

// ─── Helpers ─────────────────────────────────────────────────────────────────

fn parse_f64(s: &str) -> f64 {
    s.replace(',', "").trim().parse().unwrap_or(0.0)
}

fn parse_cas_date(s: &str) -> String {
    // "01-Jan-2024" → "2024-01-01"
    const MONTHS: &[&str] = &["jan","feb","mar","apr","may","jun","jul","aug","sep","oct","nov","dec"];
    let parts: Vec<&str> = s.splitn(3, '-').collect();
    if parts.len() == 3 {
        let day = parts[0].parse::<u32>().unwrap_or(0);
        let mon_idx = MONTHS.iter().position(|&m| m.eq_ignore_ascii_case(parts[1]));
        let year = parts[2].parse::<u32>().unwrap_or(0);
        if let (Some(m), d, y) = (mon_idx, day, year) {
            if d > 0 && y > 0 { return format!("{y}-{:02}-{d:02}", m + 1); }
        }
    }
    s.to_string()
}

fn classify_txn(desc: &str) -> &'static str {
    let d = desc.to_uppercase();
    if d.contains("REDEMPTION") || d.contains("REDEEM") { return "REDEMPTION"; }
    if d.contains("SWITCH IN")  || d.contains("SWITCH-IN")  { return "SWITCH_IN"; }
    if d.contains("SWITCH OUT") || d.contains("SWITCH-OUT") { return "SWITCH_OUT"; }
    if d.contains("DIVIDEND")   || d.contains("IDCW")       { return "DIVIDEND"; }
    if d.contains("SIP")        || d.contains("SYSTEMATIC") { return "SIP"; }
    if d.contains("PURCHASE")   || d.contains("NFO")        { return "BUY"; }
    if d.contains("BONUS")                                   { return "BONUS"; }
    "BUY"
}

fn is_non_financial(desc: &str) -> bool {
    let d = desc.to_uppercase();
    d.contains("ADDRESS") || d.contains("NOMINEE") || d.contains("KYC STATUS")
        || d.contains("CANCELLED") || d.contains("REGISTRATION OF")
        || d.contains("EMAIL") || d.contains("MOBILE") || d.contains("BANK ACCOUNT")
        || d.contains("DEREGISTR") || d.contains("CONSOLIDATION")
}

/// Extract all floating-point numbers from a string.
fn all_numbers(s: &str) -> Vec<f64> {
    number_re().find_iter(s).map(|m| parse_f64(m.as_str())).collect()
}

/// Strip CAMS page-stamp appended at page breaks (e.g. "CAMSCASWS-210426… Version:V3.4 Live-1017").
fn strip_page_stamp(s: &str) -> &str {
    if let Some(pos) = s.find("CAMSCASWS") {
        s[..pos].trim_end()
    } else {
        s
    }
}

fn is_noise_line(line: &str) -> bool {
    let lc = line.to_lowercase();
    lc.contains("consolidated account statement")
        || lc.contains("page no") || lc.contains("page 1") || lc.contains("page of")
        || lc.starts_with("www.") || lc.starts_with("http")
        || lc.contains("generated on") || lc.contains("this is a computer")
}

fn is_pan_kyc_line(line: &str) -> bool {
    let lc = line.to_uppercase();
    (lc.contains("PAN:") && lc.contains("KYC:")) || lc.starts_with("PAN: ")
}

/// Extract ISIN from a line, tolerating a single internal space caused by
/// PDF text-run boundaries (e.g. "INF179KA1RZ 8" → "INF179KA1RZ8").
fn extract_isin(line: &str) -> Option<String> {
    if let Some(cap) = isin_re().captures(line) {
        return Some(cap[1].to_string());
    }
    // Handle split ISIN: find "ISIN:" then collect next alphanumeric chars (ignoring spaces)
    let isin_pos = line.find("ISIN")?;
    let after = line[isin_pos..].find(':')?;
    let rest = line[isin_pos + after + 1..].trim_start();
    if rest.starts_with("IN") {
        let compact: String = rest.chars().take(16).filter(|c| c.is_alphanumeric()).collect();
        if compact.len() >= 12 && compact.starts_with("IN") {
            return Some(compact[..12].to_string());
        }
    }
    // Fallback: bare ISIN on a dash-containing non-date line
    if line.contains('-') && !date_re().is_match(line) {
        if let Some(cap) = bare_isin_re().captures(line) {
            return Some(cap[1].to_string());
        }
    }
    None
}

/// Extract folio number from a line, joining parts separated by spaces.
fn extract_folio(line: &str) -> Option<String> {
    let cap = folio_re().captures(line)?;
    // Collapse internal whitespace; keep "/" and digits.
    let raw = cap[1].trim().to_string();
    Some(raw)
}

/// Extract scheme name from an ISIN line.
/// "G223-Bandhan ELSS Tax saver Fund-Regular Plan-Growth (Non-Demat) - ISIN: INF..."
/// → "Bandhan ELSS Tax saver Fund-Regular Plan-Growth"
fn scheme_from_isin_line(line: &str) -> String {
    let isin_pos = line.find("ISIN").unwrap_or(line.len());
    let before = line[..isin_pos].trim_end_matches(|c: char| c == '-' || c == ' ' || c == '(');
    strip_scheme_code_prefix(before)
}

/// Returns true if line starts with a CAMS scheme code prefix like "P1191-" or "G223-".
fn starts_with_scheme_code(line: &str) -> bool {
    static RE: OnceLock<Regex> = OnceLock::new();
    RE.get_or_init(|| Regex::new(r"^\s*[A-Z][A-Z0-9]{1,6}[-/]").unwrap())
        .is_match(line)
}

/// Strip leading CAMS scheme code prefix ("G223-", "P1191-", etc.) from a line,
/// and remove trailing CAMS registrar annotation "(Non Registrar : CAMS ...)".
fn strip_scheme_code_prefix(s: &str) -> String {
    static RE: OnceLock<Regex> = OnceLock::new();
    let stripped = RE.get_or_init(|| Regex::new(r"^\s*[A-Z0-9]+[-/]").unwrap())
        .replace(s, "").trim().to_string();
    // Remove CAMS-inserted "(Non Registrar : CAMS ..." annotation
    let clean = if let Some(pos) = stripped.find("(Non Registrar") {
        stripped[..pos].trim_end().to_string()
    } else {
        stripped
    };
    clean.trim_end_matches(|c: char| c == '(' || c.is_whitespace()).to_string()
}

// ─── Column layout (PDF points) ──────────────────────────────────────────────

/// Left edge of each column (index 0 = Date, 1 = Transaction, …).
const COL_STARTS: &[f32] = &[0.0, 74.0, 328.0, 395.0, 452.0, 521.0];
/// Boundaries passed to the span extractor to force breaks between columns.
const COL_BOUNDARIES: &[f32] = &[74.0, 328.0, 395.0, 452.0, 521.0];

// Column indices
const C_DATE: usize = 0;
const C_DESC: usize = 1;
const C_AMT:  usize = 2;
const C_UNIT: usize = 3;
const C_NAV:  usize = 4;
const C_BAL:  usize = 5;

// ─── Main entry points ────────────────────────────────────────────────────────

#[tauri::command]
pub fn parse_cams_cas_pdf(file_path: String) -> Result<CasPreview, String> {
    parse_cas_spans(&file_path)
}

// ─── Import result ────────────────────────────────────────────────────────────

#[derive(Debug, Serialize)]
pub struct CasImportFundResult {
    pub isin:                  String,
    pub scheme:                String,
    pub folio:                 String,
    pub transactions_imported: usize,
    pub transactions_skipped:  usize,
    pub account_created:       bool,
    pub instrument_created:    bool,
}

#[derive(Debug, Serialize)]
pub struct CasImportResult {
    pub funds_imported:          usize,
    pub transactions_imported:   usize,
    pub transactions_skipped:    usize,
    pub instruments_auto_created: usize,
    pub fund_results:            Vec<CasImportFundResult>,
}

#[derive(Debug, Deserialize)]
pub struct CasFundAssignment {
    pub portfolio_id: i64,
    pub fund:         CasFundPreview,
}

#[derive(Debug, Deserialize)]
pub struct CasImportInput {
    pub assignments: Vec<CasFundAssignment>,
}

#[tauri::command]
pub fn import_cams_cas(input: CasImportInput) -> Result<CasImportResult, String> {
    let conn = db::acquire()?;

    let mf_type_id: i64 = conn
        .query_row(
            "SELECT instrument_type_id FROM instrument_types WHERE name='EQUITY_MF' LIMIT 1",
            [],
            |row| row.get(0),
        )
        .unwrap_or(2);

    let mut total_imported   = 0usize;
    let mut total_skipped    = 0usize;
    let mut total_auto_instr = 0usize;
    let mut fund_results     = Vec::new();

    for assignment in &input.assignments {
        let portfolio_id = assignment.portfolio_id;
        let fund = &assignment.fund;
        if fund.transactions.is_empty() { continue; }

        // ── Find or create account for this folio ─────────────────────────────
        // Match by folio number within the portfolio; fall back to creating a new account.
        let account_id: i64 = if !fund.folio.is_empty() {
            conn.query_row(
                "SELECT account_id FROM accounts
                 WHERE portfolio_id=?1 AND account_no=?2 AND account_type='MF_FOLIO' LIMIT 1",
                rusqlite::params![portfolio_id, fund.folio],
                |row| row.get(0),
            )
            .ok()
        } else {
            None
        }
        .unwrap_or_else(|| {
            let name = if !fund.scheme.is_empty() { fund.scheme.as_str() } else { &fund.isin };
            conn.execute(
                "INSERT INTO accounts (portfolio_id, name, account_type, broker, account_no)
                 VALUES (?1, ?2, 'MF_FOLIO', 'CAMS', ?3)",
                rusqlite::params![
                    portfolio_id, name,
                    if fund.folio.is_empty() { None } else { Some(&fund.folio) },
                ],
            )
            .unwrap_or(0);
            conn.last_insert_rowid()
        });

        let account_created = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions WHERE account_id=?1", [account_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(0) == 0;

        // ── Find or create instrument by ISIN ─────────────────────────────────
        let instrument_id: i64 = conn
            .query_row(
                "SELECT instrument_id FROM instruments WHERE isin=?1 LIMIT 1",
                [&fund.isin],
                |row| row.get(0),
            )
            .ok()
            .unwrap_or_else(|| {
                let name = if !fund.scheme.is_empty() { fund.scheme.as_str() } else { &fund.isin };
                conn.execute(
                    "INSERT OR IGNORE INTO instruments (isin, name, instrument_type_id, source)
                     VALUES (?1, ?2, ?3, 'IMPORT')",
                    rusqlite::params![fund.isin, name, mf_type_id],
                )
                .unwrap_or(0);
                conn.query_row(
                    "SELECT instrument_id FROM instruments WHERE isin=?1 LIMIT 1",
                    [&fund.isin],
                    |row| row.get(0),
                )
                .unwrap_or(0)
            });

        let instrument_created = conn
            .query_row(
                "SELECT COUNT(*) FROM transactions
                 WHERE instrument_id=?1 AND account_id != ?2", [instrument_id, account_id],
                |r| r.get::<_, i64>(0),
            )
            .unwrap_or(1) == 0;
        if instrument_created { total_auto_instr += 1; }

        // ── Import transactions ───────────────────────────────────────────────
        let mut imported = 0usize;
        let mut skipped  = 0usize;

        for txn in &fund.transactions {
            // Skip SWITCH_OUT (mirror of SWITCH_IN) to avoid double-counting.
            if txn.txn_type == "SWITCH_OUT" { skipped += 1; continue; }
            if txn.amount_rs == 0.0 && txn.units.abs() < 0.000_01 { skipped += 1; continue; }

            let nav_paise = (txn.nav_rs * 100.0).round() as i64;

            // Dedup: same account + instrument + date + nav + type
            let exists: bool = conn
                .query_row(
                    "SELECT COUNT(*) FROM transactions
                     WHERE account_id=?1 AND instrument_id=?2 AND trade_date=?3
                       AND price_paise=?4 AND txn_type=?5",
                    rusqlite::params![account_id, instrument_id, txn.date, nav_paise, txn.txn_type],
                    |row| row.get::<_, i64>(0),
                )
                .map(|c| c > 0)
                .unwrap_or(false);

            if exists { skipped += 1; continue; }

            let units_for_db = match txn.txn_type.as_str() {
                "REDEMPTION" | "SWITCH_OUT" => -txn.units.abs(),
                _ => txn.units.abs(),
            };
            let total_value_paise = match txn.txn_type.as_str() {
                "BUY" | "SIP" | "SWITCH_IN" => -(txn.amount_rs.abs() * 100.0).round() as i64,
                _ => (txn.amount_rs.abs() * 100.0).round() as i64,
            };
            let stamp_paise = (txn.stamp_duty_rs * 100.0).round() as i64;
            let stt_paise   = (txn.stt_rs        * 100.0).round() as i64;

            conn.execute(
                "INSERT INTO transactions
                    (account_id, instrument_id, txn_type, trade_segment, trade_date,
                     quantity, price_paise, brokerage_paise, stt_paise, other_charges_paise,
                     total_value_paise, notes)
                 VALUES (?1,?2,?3,'MF',?4,?5,?6,?7,?8,0,?9,?10)",
                rusqlite::params![
                    account_id, instrument_id, txn.txn_type, txn.date,
                    units_for_db, nav_paise, stamp_paise, stt_paise,
                    total_value_paise,
                    if fund.folio.is_empty() { None } else { Some(format!("Folio: {}", fund.folio)) },
                ],
            )
            .map_err(|e| e.to_string())?;

            imported += 1;
        }

        // Run oversell detection for this account after importing.
        crate::commands::import::flag_oversells(account_id).ok();

        total_imported += imported;
        total_skipped  += skipped;

        fund_results.push(CasImportFundResult {
            isin: fund.isin.clone(),
            scheme: fund.scheme.clone(),
            folio: fund.folio.clone(),
            transactions_imported: imported,
            transactions_skipped: skipped,
            account_created,
            instrument_created,
        });
    }

    Ok(CasImportResult {
        funds_imported:           fund_results.len(),
        transactions_imported:    total_imported,
        transactions_skipped:     total_skipped,
        instruments_auto_created: total_auto_instr,
        fund_results,
    })
}

// ─── Span-based parser ────────────────────────────────────────────────────────

fn parse_cas_spans(file_path: &str) -> Result<CasPreview, String> {
    let all_pages =
        pdf_utils::extract_all_page_spans_with_boundaries(file_path, COL_BOUNDARIES)
            .map_err(|e| format!("PDF span extraction failed: {e}"))?;

    let mut pan         = String::new();
    let mut period_from = String::new();
    let mut period_to   = String::new();
    let mut funds: Vec<CasFundPreview> = Vec::new();
    let mut state       = State::Before;
    let mut recent: Vec<String> = Vec::new();
    // Pending partial transaction: (date_iso, description_so_far, amount)
    // Used when description wraps to the next row with no new date.
    let mut pending: Option<PendingTxn> = None;

    let mut row_idx = 0usize;

    for page_spans in &all_pages {
        let page_rows = pdf_utils::page_spans_to_rows(page_spans.clone(), 5.0);

        for row in page_rows {
            if row.is_empty() { continue; }

            // Full-row text for header regex matching (strip CAMS page-stamp).
            let full_raw: String = row.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
            let full_line = strip_page_stamp(full_raw.trim());
            if full_line.is_empty() { continue; }

            // Column cells: [Date, Description, Amount, Units, NAV, Balance]
            let cells = pdf_utils::spans_to_cells(&row, COL_STARTS);
            let cell = |i: usize| cells.get(i).map(|s| s.trim()).unwrap_or("");

            // Pre-scan first ~80 rows for investor-level fields.
            row_idx += 1;
            if row_idx <= 80 {
                if pan.is_empty() {
                    if let Some(cap) = pan_re().captures(full_line) { pan = cap[1].to_string(); }
                }
                if period_from.is_empty() {
                    if let Some(cap) = period_re().captures(full_line) {
                        period_from = parse_cas_date(&cap[1]);
                        period_to   = parse_cas_date(&cap[2]);
                    }
                }
            }

            // ── Closing Unit Balance → commit fund ────────────────────────────
            if let Some(cap) = closing_re().captures(full_line) {
                let closing = parse_f64(&cap[1]);
                pending = None;
                state = commit_fund(state, closing, &mut funds);
                continue;
            }

            // ── ISIN line → start new fund ────────────────────────────────────
            let isin_found = extract_isin(full_line);

            if let Some(isin) = isin_found {
                match std::mem::replace(&mut state, State::Before) {
                    State::FundBody(f, _) | State::FundHeader(f) => funds.push(f),
                    State::Before => {}
                }
                pending = None;
                // Scheme: if the ISIN line is a wrap-continuation, the first part of the
                // scheme name will be in `recent` as a scheme-code prefixed line.
                let scheme_tail = scheme_from_isin_line(full_line);
                let scheme = if scheme_tail.is_empty()
                    || scheme_tail.starts_with('-')
                    || scheme_tail.contains("Registrar")
                    || scheme_tail.len() < 4
                {
                    // ISIN line wrapped — use the previous scheme fragment from recent,
                    // trimming any trailing orphaned open-paren from the wrap point.
                    recent.iter().rev()
                        .find(|l| starts_with_scheme_code(l))
                        .map(|l| {
                            let s = strip_scheme_code_prefix(l);
                            s.trim_end_matches(|c: char| c == '(' || c.is_whitespace()).to_string()
                        })
                        .unwrap_or(scheme_tail)
                } else {
                    scheme_tail
                };
                let amc = recent.iter().rev()
                    .find(|l| !is_pan_kyc_line(l) && !is_noise_line(l) && l.len() > 2
                              && !starts_with_scheme_code(l))
                    .cloned()
                    .unwrap_or_default();
                // Folio/PAN may appear before ISIN in visual order — scan recent buffer
                let folio = recent.iter().rev()
                    .find_map(|l| extract_folio(l))
                    .unwrap_or_default();
                let pan = recent.iter().rev()
                    .find_map(|l| pan_re().captures(l).map(|c| c[1].to_string()))
                    .unwrap_or_default();
                state = State::FundHeader(CasFundPreview {
                    amc, scheme, isin, folio, pan,
                    opening_balance: 0.0, closing_balance: 0.0, transactions: Vec::new(),
                });
                recent.clear();
                continue;
            }

            // ── Fund header ───────────────────────────────────────────────────
            if let State::FundHeader(ref mut fund) = state {
                if fund.folio.is_empty() {
                    if let Some(cap) = folio_re().captures(full_line) {
                        fund.folio = cap[1].trim_end_matches(|c: char| !c.is_alphanumeric()).to_string();
                    }
                }
                if fund.pan.is_empty() {
                    if let Some(cap) = pan_re().captures(full_line) {
                        fund.pan = cap[1].to_string();
                    }
                }
                if let Some(cap) = opening_re().captures(full_line) {
                    fund.opening_balance = parse_f64(&cap[1]);
                    let f = match std::mem::replace(&mut state, State::Before) {
                        State::FundHeader(f) => f,
                        other => { state = other; continue; }
                    };
                    let opening = f.opening_balance;
                    state = State::FundBody(f, opening);
                }
                if !is_noise_line(full_line) && full_line.len() > 2 {
                    recent.push(full_line.to_string());
                    if recent.len() > 12 { recent.remove(0); }
                }
                continue;
            }

            // ── Fund body ─────────────────────────────────────────────────────
            if let State::FundBody(ref mut fund, ref mut prev_bal) = state {

                // Stamp duty / STT — attach to last transaction.
                if let Some(cap) = stamp_duty_re().captures(full_line) {
                    if let Some(t) = fund.transactions.last_mut() {
                        t.stamp_duty_rs = parse_f64(&cap[1]);
                    }
                    pending = None;
                    continue;
                }
                if let Some(cap) = stt_re().captures(full_line) {
                    if let Some(t) = fund.transactions.last_mut() {
                        t.stt_rs = parse_f64(&cap[1]);
                    }
                    pending = None;
                    continue;
                }

                // Column header row — skip.
                {
                    let lc = full_line.to_lowercase();
                    if lc.contains("transaction") && (lc.contains("amount") || lc.contains("nav")) {
                        pending = None;
                        continue;
                    }
                }

                let date_cell = cell(C_DATE);
                let bal_str   = normalise_signs(cell(C_BAL));
                let amt_str   = normalise_signs(cell(C_AMT));

                // ── Transaction row: date present ─────────────────────────────
                if let Some(cap) = date_re().captures(date_cell) {
                    let raw_date = cap[1].to_string();

                    // Text after the date that overflowed into the date cell
                    let date_overflow = cap[2].trim().to_string();
                    let desc_raw = {
                        let d = cell(C_DESC);
                        if date_overflow.is_empty() { d.to_string() }
                        else if d.is_empty() { date_overflow.clone() }
                        else { format!("{date_overflow} {d}") }
                    };

                    if is_non_financial(&desc_raw) { pending = None; continue; }

                    let has_balance = !bal_str.is_empty();
                    let balance     = if has_balance { parse_f64(&bal_str) } else { 0.0 };
                    let amount      = parse_f64(&amt_str);

                    if !has_balance {
                        // Numbers not yet on this row — pend and wait for continuation.
                        pending = Some(PendingTxn {
                            date: raw_date,
                            desc: desc_raw,
                            amount,
                        });
                        continue;
                    }

                    // Resolve any stale pending (different date = discard it).
                    pending = None;

                    let units = resolve_units(cell(C_UNIT), balance, *prev_bal);
                    let nav   = resolve_nav(cell(C_NAV), amount, units);

                    if amount == 0.0 && units.abs() < 0.000_01 { continue; }

                    *prev_bal = balance;
                    fund.transactions.push(make_txn(&raw_date, desc_raw, amount, units, nav, balance));
                    continue;
                }

                // ── No-date row: continuation or noise ───────────────────────
                if is_noise_line(full_line) { continue; }

                // Description continuation: pending txn, current row has balance.
                if let Some(ref mut p) = pending {
                    if !bal_str.is_empty() {
                        let balance = parse_f64(&bal_str);
                        let amount  = if !amt_str.is_empty() { parse_f64(&amt_str) } else { p.amount };
                        let desc    = if !cell(C_DESC).is_empty() {
                            format!("{} {}", p.desc, cell(C_DESC))
                        } else {
                            p.desc.clone()
                        };
                        let units = resolve_units(cell(C_UNIT), balance, *prev_bal);
                        let nav   = resolve_nav(cell(C_NAV), amount, units);

                        if !(amount == 0.0 && units.abs() < 0.000_01) {
                            let raw_date = p.date.clone();
                            *prev_bal = balance;
                            fund.transactions.push(make_txn(&raw_date, desc, amount, units, nav, balance));
                        }
                        pending = None;
                        continue;
                    }
                    // Still no balance — accumulate description.
                    if !cell(C_DESC).is_empty() {
                        p.desc.push(' ');
                        p.desc.push_str(cell(C_DESC));
                    }
                }
                continue;
            }

            // ── Before any fund — buffer for AMC lookahead ───────────────────
            if !is_noise_line(full_line) && full_line.len() > 2 {
                recent.push(full_line.to_string());
                if recent.len() > 12 { recent.remove(0); }
            }
        }
    }

    // Commit dangling fund.
    match state {
        State::FundBody(f, _) | State::FundHeader(f) => funds.push(f),
        State::Before => {}
    }

    let total_transactions = funds.iter().map(|f| f.transactions.len()).sum();
    Ok(CasPreview { investor_name: String::new(), pan, period_from, period_to, funds, total_transactions })
}

// ─── Parser helpers ───────────────────────────────────────────────────────────

struct PendingTxn {
    date:   String,
    desc:   String,
    amount: f64,
}

fn commit_fund(state: State, closing: f64, funds: &mut Vec<CasFundPreview>) -> State {
    match state {
        State::FundHeader(mut f) => { f.closing_balance = closing; funds.push(f); }
        State::FundBody(mut f, _) => { f.closing_balance = closing; funds.push(f); }
        State::Before => {}
    }
    State::Before
}

/// Units from the Units cell; fall back to balance-delta if cell is empty/zero.
fn resolve_units(units_cell: &str, balance: f64, prev_bal: f64) -> f64 {
    if !units_cell.is_empty() {
        let u = parse_f64(&normalise_signs(units_cell));
        if u.abs() > 0.000_01 { return u; }
    }
    balance - prev_bal
}

/// NAV from the NAV cell; fall back to |amount| / |units| if cell is empty/zero.
fn resolve_nav(nav_cell: &str, amount: f64, units: f64) -> f64 {
    if !nav_cell.is_empty() {
        let n = parse_f64(nav_cell);
        if n > 0.0 { return n; }
    }
    if units.abs() > 0.000_01 { amount.abs() / units.abs() } else { 0.0 }
}

fn make_txn(raw_date: &str, desc: String, amount: f64, units: f64, nav: f64, balance: f64) -> CasTransaction {
    CasTransaction {
        date:          parse_cas_date(raw_date),
        txn_type:      classify_txn(&desc).to_string(),
        description:   desc,
        amount_rs:     amount,
        units,
        nav_rs:        nav,
        unit_balance:  balance,
        stamp_duty_rs: 0.0,
        stt_rs:        0.0,
    }
}

// ─── Parser ───────────────────────────────────────────────────────────────────

enum State {
    /// Outside any fund section, buffering recent lines for AMC lookahead
    Before,
    /// Inside a fund section but still in the header (waiting for Opening Unit Balance)
    FundHeader(CasFundPreview),
    /// Actively parsing transactions; second field is the running unit balance
    FundBody(CasFundPreview, f64),
}

fn parse_cas_text(text: &str) -> Result<CasPreview, String> {
    let lines: Vec<&str> = text.lines().collect();

    let mut pan         = String::new();
    let mut period_from = String::new();
    let mut period_to   = String::new();

    // Pre-scan first 60 lines for investor-level fields
    for raw in lines.iter().take(60) {
        let line = raw.trim();
        if line.is_empty() { continue; }
        if let Some(cap) = pan_re().captures(line) {
            if pan.is_empty() { pan = cap[1].to_string(); }
        }
        if let Some(cap) = period_re().captures(line) {
            if period_from.is_empty() {
                period_from = parse_cas_date(&cap[1]);
                period_to   = parse_cas_date(&cap[2]);
            }
        }
    }

    let mut funds: Vec<CasFundPreview> = Vec::new();
    let mut state  = State::Before;
    // Rolling buffer of recent non-noise lines for AMC name lookahead
    let mut recent: Vec<String> = Vec::new();
    // Pending partial transaction when description wraps to the next line
    let mut pending: Option<(String, String)> = None; // (date, description_so_far)

    for raw in &lines {
        let line = raw.trim();
        if line.is_empty() { continue; }

        // ── Closing Unit Balance → commit current fund ────────────────────────
        if let Some(cap) = closing_re().captures(line) {
            let closing = parse_f64(&cap[1]);
            pending = None;
            match state {
                State::FundHeader(mut f) => {
                    f.closing_balance = closing;
                    funds.push(f);
                    state = State::Before;
                    recent.clear();
                }
                State::FundBody(mut f, _) => {
                    f.closing_balance = closing;
                    funds.push(f);
                    state = State::Before;
                    recent.clear();
                }
                State::Before => {}
            }
            continue;
        }

        // ── ISIN line → start new fund ────────────────────────────────────────
        // Primary: "ISIN: INFxxx" label; fallback: bare ISIN on a line that
        // also has a dash (scheme-code separator), filtering out transaction lines.
        let isin_on_line = isin_re().captures(line)
            .map(|c| c[1].to_string())
            .or_else(|| {
                if line.contains('-') && !date_re().is_match(line) {
                    bare_isin_re().captures(line).map(|c| c[1].to_string())
                } else {
                    None
                }
            });
        if let Some(isin) = isin_on_line {
            match state {
                State::FundBody(f, _) | State::FundHeader(f) => funds.push(f),
                State::Before => {}
            }
            pending = None;

            let scheme = scheme_from_isin_line(line);

            let amc = recent.iter().rev()
                .find(|l| !is_pan_kyc_line(l) && !is_noise_line(l) && l.len() > 2)
                .cloned()
                .unwrap_or_default();

            state = State::FundHeader(CasFundPreview {
                amc, scheme, isin,
                folio: String::new(), pan: String::new(),
                opening_balance: 0.0, closing_balance: 0.0, transactions: Vec::new(),
            });
            recent.clear();
            continue;
        }

        // ── Inside fund header ────────────────────────────────────────────────
        if let State::FundHeader(ref mut fund) = state {
            if let Some(cap) = folio_re().captures(line) {
                if fund.folio.is_empty() {
                    fund.folio = cap[1].trim_end_matches(|c: char| !c.is_alphanumeric()).to_string();
                }
            }
            if let Some(cap) = opening_re().captures(line) {
                fund.opening_balance = parse_f64(&cap[1]);
                let f = match std::mem::replace(&mut state, State::Before) {
                    State::FundHeader(f) => f,
                    other => { state = other; continue; }
                };
                let opening = f.opening_balance;
                state = State::FundBody(f, opening);
            }
            continue;
        }

        // ── Inside fund body ──────────────────────────────────────────────────
        if let State::FundBody(ref mut fund, ref mut prev_bal) = state {
            // Stamp duty / STT — attach to last transaction
            if let Some(cap) = stamp_duty_re().captures(line) {
                if let Some(t) = fund.transactions.last_mut() {
                    t.stamp_duty_rs = parse_f64(&cap[1]);
                }
                pending = None;
                continue;
            }
            if let Some(cap) = stt_re().captures(line) {
                if let Some(t) = fund.transactions.last_mut() {
                    t.stt_rs = parse_f64(&cap[1]);
                }
                pending = None;
                continue;
            }

            // Transaction row starts with DD-Mon-YYYY
            if let Some(cap) = date_re().captures(line) {
                let raw_date = cap[1].to_string();
                let rest     = cap[2].trim().to_string();

                if is_non_financial(&rest) { pending = None; continue; }
                if rest.contains("Stamp Duty") || rest.contains("STT Paid") { continue; }

                // Strip CAMS page-stamp appended at page breaks, then normalise parens
                let rest_clean = strip_page_stamp(&rest).to_string();
                let rest_n = normalise_signs(&rest_clean);
                let matches: Vec<_> = number_re().find_iter(&rest_n).collect();

                if matches.len() >= 2 {
                    pending = None;
                    let n = matches.len();

                    let amount  = parse_f64(matches[0].as_str());
                    let balance = parse_f64(matches[n - 1].as_str());

                    // Units derived from running balance delta — avoids merged-column ambiguity
                    let units = balance - *prev_bal;
                    // NAV computed: |amount| / |units|
                    let nav = if units.abs() > 0.000_01 { amount.abs() / units.abs() } else { 0.0 };

                    if amount == 0.0 && units.abs() < 0.000_01 { continue; }

                    // Description: from first alphabetic char after amount to start of balance
                    let first_num_end = matches[0].end();
                    let desc_start = rest_n[first_num_end..]
                        .find(|c: char| c.is_alphabetic())
                        .map(|i| first_num_end + i)
                        .unwrap_or(first_num_end);
                    let desc_end = matches[n - 1].start();
                    let desc = if desc_end > desc_start {
                        rest_n[desc_start..desc_end].trim().to_string()
                    } else {
                        rest.clone()
                    };

                    *prev_bal = balance;
                    fund.transactions.push(CasTransaction {
                        date:          parse_cas_date(&raw_date),
                        txn_type:      classify_txn(&desc).to_string(),
                        description:   desc,
                        amount_rs:     amount,
                        units,
                        nav_rs:        nav,
                        unit_balance:  balance,
                        stamp_duty_rs: 0.0,
                        stt_rs:        0.0,
                    });
                } else {
                    // Only 0 or 1 number — description or date-only row; skip or pend
                    pending = Some((raw_date, rest));
                }
                continue;
            }

            // No-date line — could be column header or footnote; skip
            {
                let lc = line.to_lowercase();
                if lc.contains("transaction") && (lc.contains("amount") || lc.contains("nav")) {
                    pending = None;
                    continue;
                }
            }
            // Continuation of a multi-line description (no new numeric data expected here)
            if let Some((date, desc)) = pending.take() {
                let combined = format!("{} {}", desc, line.trim());
                pending = Some((date, combined));
            }
            continue;
        }

        // ── Before any fund section — buffer for AMC lookahead ───────────────
        if !is_noise_line(line) && line.len() > 2 {
            recent.push(line.to_string());
            if recent.len() > 12 { recent.remove(0); }
        }
    }

    // Commit dangling fund (missing Closing Unit Balance)
    match state {
        State::FundBody(f, _) | State::FundHeader(f) => funds.push(f),
        State::Before => {}
    }

    let total_transactions = funds.iter().map(|f| f.transactions.len()).sum();

    Ok(CasPreview {
        investor_name: String::new(),
        pan,
        period_from,
        period_to,
        funds,
        total_transactions,
    })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    fn run(path: &str) {
        if !std::path::Path::new(path).exists() {
            eprintln!("SKIP: file not found: {path}");
            return;
        }
        let result = parse_cams_cas_pdf(path.to_string()).expect("parse failed");
        println!("File: {path}");
        println!("  PAN: {}  Period: {} → {}", result.pan, result.period_from, result.period_to);
        println!("  Funds: {}  Total transactions: {}", result.funds.len(), result.total_transactions);
        println!();
        for fund in &result.funds {
            println!("  ┌─ AMC: {}  Scheme: {}", fund.amc, fund.scheme);
            println!("  │  ISIN: {}  Folio: {}  Open: {:.3}  Close: {:.3}",
                fund.isin, fund.folio, fund.opening_balance, fund.closing_balance);
            for t in &fund.transactions {
                println!("  │  {} {:12} {:>10.2}  units {:>9.4}  nav {:>8.4}  bal {:>10.4}  stamp={:.2}  stt={:.2}",
                    t.date, t.txn_type, t.amount_rs, t.units, t.nav_rs, t.unit_balance,
                    t.stamp_duty_rs, t.stt_rs);
            }
            println!("  └─ {} txns", fund.transactions.len());
            println!();
        }
        assert!(result.funds.len() > 0, "expected at least one fund");
        assert!(result.total_transactions > 0, "expected transactions");
    }

    #[test]
    fn test_raw_text() {
        let path = "/home/dmachine/Downloads/CAMS_Report.pdf";
        if !std::path::Path::new(path).exists() { return; }
        let text = pdf_extract::extract_text(path).expect("extract failed");
        for (i, line) in text.lines().enumerate().take(300) {
            println!("{:4}: {}", i, line);
        }
    }

    #[test]
    fn test_sbi_lines() {
        let path = "/home/dmachine/Downloads/CAMS_Report.pdf";
        if !std::path::Path::new(path).exists() { return; }
        let text = pdf_extract::extract_text(path).expect("extract failed");
        let mut in_sbi = false;
        for (i, line) in text.lines().enumerate() {
            if line.contains("INF200K01495") || line.contains("SBI ELSS") {
                in_sbi = true;
            }
            if in_sbi {
                println!("{:4}: {}", i, line);
                if line.contains("Closing Unit Balance") { break; }
            }
        }
    }

    #[test]
    fn test_nippon_lines() {
        let path = "/home/dmachine/Downloads/CAMS_Report.pdf";
        if !std::path::Path::new(path).exists() { return; }
        let text = pdf_extract::extract_text(path).expect("extract failed");
        let mut in_nippon = false;
        for (i, line) in text.lines().enumerate() {
            if line.contains("NIPPON INDIA MULTI CAP") || line.contains("INF204K01489") {
                in_nippon = true;
            }
            if in_nippon {
                println!("{:4}: {}", i, line);
                if line.contains("Closing Unit Balance") { break; }
            }
        }
    }

    #[test]
    fn test_cas_report() {
        run("/home/dmachine/Downloads/CAMS_Report.pdf");
    }

    #[test]
    fn test_span_rows() {
        let path = "/home/dmachine/Downloads/CAMS_Report.pdf";
        if !std::path::Path::new(path).exists() { return; }
        let pages = pdf_utils::extract_all_page_spans_with_boundaries(path, COL_BOUNDARIES)
            .expect("span extraction failed");
        let mut count = 0;
        'outer: for (pi, page) in pages.iter().enumerate() {
            let rows = pdf_utils::page_spans_to_rows(page.clone(), 5.0);
            for row in &rows {
                let full: String = row.iter().map(|s| s.text.as_str()).collect::<Vec<_>>().join(" ");
                let full = strip_page_stamp(full.trim()).to_string();
                let cells = pdf_utils::spans_to_cells(row, COL_STARTS);
                let g = |i: usize| cells.get(i).map(|s| s.as_str()).unwrap_or("");
                println!("p{} FULL: {}", pi+1, &full);
                println!("     DATE={:20} AMT={:14} BAL={}",
                    g(C_DATE).chars().take(20).collect::<String>(),
                    g(C_AMT).chars().take(14).collect::<String>(),
                    g(C_BAL));
                count += 1;
                if count >= 60 { break 'outer; }
            }
        }
    }

    /// Column boundaries measured from the CAMS CAS header row (PDF points).
    const COLS: &'static [(&'static str, f32, f32, &'static str)] = &[
        ("Date",        0.0,   74.0,  "#d0e8ff"),
        ("Transaction", 74.0,  328.0, "#d0ffd8"),
        ("Amount",      328.0, 395.0, "#fff3d0"),
        ("Units",       395.0, 452.0, "#ffd0d0"),
        ("Price/NAV",   452.0, 521.0, "#f0d0ff"),
        ("Unit Bal",    521.0, f32::MAX, "#d0f0ff"),
    ];

    fn col_for(x: f32) -> usize {
        for (i, &(_, lo, hi, _)) in COLS.iter().enumerate() {
            if x >= lo && x < hi { return i; }
        }
        COLS.len() - 1
    }

    /// Three-panel HTML visualization.  Output: /tmp/cams_spans.html
    ///
    /// Left   — spans reconstructed in reading order (rows top→bottom, spans left→right).
    /// Right  — pdf_extract::extract_text() output.
    /// Bottom — positioned span map with column colour bands and dividers.
    #[test]
    fn test_visualise_spans() {
        let pdf_path = "/home/dmachine/Downloads/CAMS_Report.pdf";
        if !std::path::Path::new(pdf_path).exists() {
            eprintln!("SKIP: {pdf_path} not found");
            return;
        }

        // Use boundary-aware extraction so negative values like "(25,974.81)"
        // don't merge with the preceding column's text.
        let col_xs: Vec<f32> = COLS.iter().skip(1).map(|&(_, lo, _, _)| lo).collect();
        let pages = pdf_utils::extract_all_page_spans_with_boundaries(pdf_path, &col_xs)
            .expect("span extraction failed");

        // ── 1. Span-based reading-order reconstruction ────────────────────────
        let mut span_lines: Vec<String> = Vec::new();
        for (page_idx, spans) in pages.iter().enumerate() {
            span_lines.push(format!("=== Page {} ===", page_idx + 1));
            let rows = pdf_utils::page_spans_to_rows(spans.clone(), 5.0);
            for row in &rows {
                let mut line = String::new();
                let mut prev_right = f32::NEG_INFINITY;
                for span in row {
                    if prev_right > f32::NEG_INFINITY && span.x - prev_right > 4.0 {
                        line.push(' ');
                    }
                    line.push_str(&span.text);
                    prev_right = span.right;
                }
                if !line.trim().is_empty() {
                    span_lines.push(line);
                }
            }
        }

        // ── 2. pdf_extract plain text ─────────────────────────────────────────
        let extract_text = pdf_extract::extract_text(pdf_path)
            .expect("extract_text failed");
        let extract_lines: Vec<&str> = extract_text.lines().collect();

        // ── 3. Positioned span map ────────────────────────────────────────────
        const SCALE: f32 = 1.33;
        let mut map_html = String::new();
        for (page_idx, spans) in pages.iter().enumerate() {
            if spans.is_empty() { continue; }
            let max_y = spans.iter().map(|s| s.y).fold(f32::NEG_INFINITY, f32::max);
            let max_x = spans.iter().map(|s| s.right).fold(f32::NEG_INFINITY, f32::max);
            let page_h = (max_y + 40.0) * SCALE;
            let page_w = (max_x + 20.0) * SCALE;

            map_html.push_str(&format!(
                "<div class='page' style='width:{page_w:.0}px;height:{page_h:.0}px'>\
                 <div class='page-label'>Page {}</div>\n",
                page_idx + 1
            ));

            // ── Column background bands + dividers ────────────────────────────
            for &(col_label, lo, hi, colour) in COLS {
                let bx = lo * SCALE;
                let bw = (hi.min(max_x + 20.0_f32) - lo).max(0.0) * SCALE;
                // band
                map_html.push_str(&format!(
                    "<div style='position:absolute;left:{bx:.0}px;top:0;\
                     width:{bw:.0}px;height:{page_h:.0}px;\
                     background:{colour};opacity:.4;pointer-events:none'></div>\n"
                ));
                // label
                map_html.push_str(&format!(
                    "<div style='position:absolute;left:{bx:.0}px;top:4px;\
                     font-size:7px;color:#555;font-weight:bold;z-index:2;\
                     pointer-events:none'>{col_label}</div>\n"
                ));
                // divider line (skip leftmost)
                if lo > 0.0 {
                    map_html.push_str(&format!(
                        "<div style='position:absolute;left:{bx:.0}px;top:0;\
                         width:1px;height:{page_h:.0}px;\
                         background:rgba(0,0,0,.2);pointer-events:none;z-index:1'></div>\n"
                    ));
                }
            }

            // ── Spans coloured by column ──────────────────────────────────────
            let rows = pdf_utils::page_spans_to_rows(spans.clone(), 5.0);
            for row in &rows {
                for span in row {
                    let css_x = span.x * SCALE;
                    let css_y = (max_y - span.y) * SCALE;
                    let span_w = (span.right - span.x + 4.0) * SCALE; // +4 px breathing room
                    let &(_, _, _, colour) = &COLS[col_for(span.x)];
                    let txt = span.text
                        .replace('&', "&amp;")
                        .replace('<', "&lt;")
                        .replace('>', "&gt;");
                    map_html.push_str(&format!(
                        "<div class='sp' style='left:{css_x:.1}px;top:{css_y:.1}px;\
                         width:{span_w:.0}px;background:{colour}'>{txt}</div>\n"
                    ));
                }
            }
            map_html.push_str("</div>\n");
        }

        // ── Assemble HTML ─────────────────────────────────────────────────────
        fn numbered_pre(lines: &[impl AsRef<str>]) -> String {
            let mut out = String::from("<pre class='code'>");
            for (i, l) in lines.iter().enumerate() {
                let l = l.as_ref()
                    .replace('&', "&amp;")
                    .replace('<', "&lt;")
                    .replace('>', "&gt;");
                out.push_str(&format!(
                    "<span class='ln'>{:>4}</span> {l}\n", i + 1
                ));
            }
            out.push_str("</pre>");
            out
        }

        let span_pre   = numbered_pre(&span_lines);
        let extract_pre = numbered_pre(&extract_lines);

        let html = format!(r#"<!DOCTYPE html>
<html><head><meta charset='utf-8'>
<style>
*{{box-sizing:border-box}}
body{{margin:0;background:#1e1e1e;font-family:monospace;color:#ccc}}
h2{{margin:0;padding:8px 12px;font-size:13px;background:#2d2d2d;color:#aaa}}
.panels{{display:flex;height:50vh;border-bottom:2px solid #444}}
.panel{{flex:1;overflow:auto;border-right:1px solid #444}}
.panel:last-child{{border-right:none}}
.code{{margin:0;padding:8px;font-size:11px;line-height:1.5;white-space:pre}}
.ln{{display:inline-block;width:3em;color:#555;user-select:none;
     border-right:1px solid #333;margin-right:6px;text-align:right}}
.map-wrap{{overflow:auto;padding:20px;background:#333}}
.page{{position:relative;background:white;margin:20px auto;
       box-shadow:0 4px 12px #0008}}
.page-label{{position:absolute;top:2px;left:4px;font-size:9px;color:#888}}
.sp{{position:absolute;white-space:nowrap;font-size:8px;
     border:1px solid rgba(0,80,200,.4);color:#111;
     padding:0 1px;transform:translateY(-100%);
     overflow:visible;min-width:fit-content}}
</style></head><body>
<div class='panels'>
  <div class='panel'><h2>Span-based (reading order)</h2>{span_pre}</div>
  <div class='panel'><h2>pdf_extract::extract_text</h2>{extract_pre}</div>
</div>
<h2>Positioned span map (alternating row colours)</h2>
<div class='map-wrap'>{map_html}</div>
</body></html>
"#);

        let out = "/tmp/cams_spans.html";
        std::fs::write(out, &html).expect("write failed");
        println!("Written: {out}  ({} pages, {} total spans, {} extract lines)",
            pages.len(),
            pages.iter().map(|p| p.len()).sum::<usize>(),
            extract_lines.len());
    }
}
