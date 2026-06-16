//! Parser for Nirmal Bang Securities — Contract Note PDF.
//!
//! These PDFs use AES-128 / V4-R4 encryption with cross-reference streams
//! (PDF 1.7).  lopdf 0.38 fails to resolve the page tree for this format.
//! We fall back to pdftotext (bundled with Git for Windows / Poppler) for text
//! extraction and parse the -layout mode output.
//!
//! Key text patterns:
//!   Metadata  — "Contract Note No. : XXXX"  "Trade Date DD-MM-YYYY"
//!   Trades    — "ISIN : IN..."  then  "Sell Average / Buy Average  qty ... net_wap  total"
//!   Charges   — "Net Payin and Payout Summary" → "Product" row (STT/exch/SEBI/other)
//!               "B/f Total  CGST  SGST" section → "Total (Net)" row

use crate::commands::import::{common, flag_oversells};
use crate::commands::import::cn_choice_equity::ParsedCharge;
use crate::db;
use serde::{Deserialize, Serialize};

// ─── Public types ─────────────────────────────────────────────────────────────

#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct ParsedNBTrade {
    pub isin:          String,
    pub security_name: String,
    pub buy_sell:      String,  // "BUY" | "SELL"
    pub quantity:      f64,
    pub price:         f64,     // WAP net of brokerage (rupees)
}

#[derive(Serialize, Deserialize, Clone)]
pub struct NBCnParseResult {
    pub trade_date:    String,
    pub cn_number:     String,
    pub client_code:   Option<String>,
    pub trades:        Vec<ParsedNBTrade>,
    pub charges:       Vec<ParsedCharge>,
    pub pages_scanned: usize,
    pub _file_path:    String,
}

#[derive(Serialize)]
pub struct NBCnImportResult {
    pub imported:                 usize,
    pub skipped:                  usize,
    pub auto_created_instruments: usize,
}

// ─── pdftotext discovery ──────────────────────────────────────────────────────

fn find_pdftotext() -> Option<String> {
    // 1. Next to the running executable — this is where Tauri places bundled sidecars on Windows.
    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("pdftotext.exe");
            if candidate.exists() {
                return Some(candidate.to_string_lossy().into_owned());
            }
        }
    }
    // 2. Common developer / CI install locations (useful during development and testing)
    let candidates: &[&str] = &[
        r"C:\Program Files\Git\mingw64\bin\pdftotext.exe",
        r"C:\Program Files (x86)\Git\mingw64\bin\pdftotext.exe",
        r"C:\poppler\bin\pdftotext.exe",
        r"C:\Program Files\poppler\bin\pdftotext.exe",
        r"C:\Program Files (x86)\poppler\bin\pdftotext.exe",
        r"C:\xpdf\bin64\pdftotext.exe",
        r"C:\Program Files\Xpdf\bin64\pdftotext.exe",
    ];
    for &p in candidates {
        if std::path::Path::new(p).exists() {
            return Some(p.to_string());
        }
    }
    // 3. Try PATH
    if std::process::Command::new("pdftotext")
        .arg("-v")
        .output()
        .map(|o| o.status.success())
        .unwrap_or(false)
    {
        return Some("pdftotext".to_string());
    }
    None
}

fn extract_text(pdf_path: &str, password: &str) -> Result<String, String> {
    let exe = find_pdftotext().ok_or_else(|| {
        "PDFTOTEXT_NOT_FOUND: pdftotext is required to import Nirmal Bang CNs. \
         It is bundled with Git for Windows — install from git-scm.com."
            .to_string()
    })?;

    let out = std::process::Command::new(&exe)
        .args(["-layout", "-q", "-upw", password, pdf_path, "-"])
        .output()
        .map_err(|e| format!("pdftotext launch failed: {e}"))?;

    if out.stdout.is_empty() {
        let err = String::from_utf8_lossy(&out.stderr);
        return Err(format!("pdftotext produced no output: {err}"));
    }

    // Accept text even with non-UTF8 bytes (special fonts, etc.)
    Ok(String::from_utf8_lossy(&out.stdout).into_owned())
}

// ─── Number helpers ───────────────────────────────────────────────────────────

/// Parse an Indian-format number: "3,35,235.11" → 335235.11.
fn parse_number(s: &str) -> Option<f64> {
    // Keep only digits, minus, and decimal point
    let clean: String = s
        .chars()
        .filter(|c| *c == '-' || *c == '.' || c.is_ascii_digit())
        .collect();
    clean.parse::<f64>().ok()
}

/// Extract all numbers from a line (Indian number format supported).
fn numbers_in_line(line: &str) -> Vec<f64> {
    static RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        // Match: optional minus, digits + commas, then either decimal or end
        regex::Regex::new(r"-?[\d,]+\.[\d]+-?[\d,]+|-?[\d,]+").unwrap()
    });
    RE.find_iter(line)
        .filter_map(|m| parse_number(m.as_str()))
        .collect()
}

fn to_paise(rupees: f64) -> i64 {
    (rupees.abs() * 100.0).round() as i64
}

// ─── Text parsing ─────────────────────────────────────────────────────────────

fn parse_metadata(text: &str) -> (String, String) {
    static CN_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"Contract Note No\.\s*:\s*(\d+)").unwrap()
    });
    static DATE_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"Trade Date\s+(\d{2}-\d{2}-\d{4})").unwrap()
    });

    let cn = CN_RE
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| m.as_str().to_string())
        .unwrap_or_default();

    let trade_date = DATE_RE
        .captures(text)
        .and_then(|c| c.get(1))
        .map(|m| {
            let parts: Vec<&str> = m.as_str().split('-').collect();
            if parts.len() == 3 {
                format!("{}-{}-{}", parts[2], parts[1], parts[0])
            } else {
                m.as_str().to_string()
            }
        })
        .unwrap_or_default();

    (cn, trade_date)
}

fn parse_trades(text: &str) -> Vec<ParsedNBTrade> {
    // ISIN marker in detail pages: "ISIN : INE609A01010"
    static ISIN_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"ISIN\s*:\s*(IN[A-Z0-9]{10})").unwrap()
    });
    // Order/trade line: BSE/NSE + long order no + time + trade no + time + security name
    static ORDER_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(
            r"^(?:BSE|NSE|MCX)\s+\d{8,}\s+\d{2}:\d{2}:\d{2}\s+\d+\s+\d{2}:\d{2}:\d{2}\s+(.+?)\s{3,}"
        ).unwrap()
    });

    let mut trades: Vec<ParsedNBTrade> = Vec::new();
    let mut current_isin = String::new();
    let mut current_name = String::new();

    for line in text.lines() {
        let trimmed = line.trim();

        // Pick up security name from order lines (appear before the ISIN marker)
        if let Some(cap) = ORDER_RE.captures(trimmed) {
            let name = cap[1].trim().to_string();
            if !name.is_empty() {
                current_name = name;
            }
        }

        // ISIN marker — start of a new security block
        if let Some(cap) = ISIN_RE.captures(trimmed) {
            current_isin = cap[1].to_string();
        }

        if current_isin.is_empty() {
            continue;
        }

        let side = if trimmed.contains("Sell Average") {
            Some("SELL")
        } else if trimmed.contains("Buy Average") {
            Some("BUY")
        } else {
            None
        };

        if let Some(bs) = side {
            let nums = numbers_in_line(line);
            // Format: "Sell Average   qty   [blanks...]   net_wap   total"
            // nums[0]=qty, nums[last-1]=net_wap, nums[last]=total
            if nums.len() >= 3 {
                let qty     = nums[0];
                let net_wap = nums[nums.len() - 2];
                if qty > 0.0 && net_wap > 0.0 {
                    trades.push(ParsedNBTrade {
                        isin:          current_isin.clone(),
                        security_name: current_name.clone(),
                        buy_sell:      bs.to_string(),
                        quantity:      qty,
                        price:         net_wap,
                    });
                }
            }
        }
    }

    trades
}

fn parse_charges(text: &str) -> Vec<ParsedCharge> {
    let mut charges: Vec<ParsedCharge> = Vec::new();
    let mut in_payout  = false;
    let mut in_gst_tbl = false;
    let mut product_done = false;

    // ── Pattern: "^\s+Product\s" (the consolidated charge row in payout summary)
    static PRODUCT_RE: once_cell::sync::Lazy<regex::Regex> = once_cell::sync::Lazy::new(|| {
        regex::Regex::new(r"^\s+Product\s").unwrap()
    });

    for line in text.lines() {
        if line.contains("Net Payin and Payout Summary") {
            in_payout = true;
        }
        if line.contains("B/f Total") && line.contains("CGST") {
            in_gst_tbl = true;
        }

        // ── Payout summary "Product" row ──────────────────────────────────────
        // Columns (equity delivery sell, 6 values):
        //   [PayIn, STT, ExchangeTrans, SEBI, Other, CfTotal]
        // With Stamp Duty (equity delivery buy, 7 values):
        //   [PayIn, STT, ExchangeTrans, SEBI, Stamp, Other, CfTotal]
        if in_payout && !in_gst_tbl && !product_done && PRODUCT_RE.is_match(line) {
            let nums = numbers_in_line(line);
            match nums.len() {
                6 => {
                    let stt  = nums[1]; let exch = nums[2];
                    let sebi = nums[3]; let oth  = nums[4];
                    if stt  > 0.001 { charges.push(ParsedCharge { charge_type: "STT".into(),              amount_paise: to_paise(stt)  }); }
                    if exch > 0.001 { charges.push(ParsedCharge { charge_type: "EXCHANGE_CHARGES".into(), amount_paise: to_paise(exch) }); }
                    if sebi > 0.001 { charges.push(ParsedCharge { charge_type: "SEBI_FEES".into(),        amount_paise: to_paise(sebi) }); }
                    if oth  > 0.001 { charges.push(ParsedCharge { charge_type: "OTHER".into(),            amount_paise: to_paise(oth)  }); }
                }
                7 => {
                    let stt   = nums[1]; let exch  = nums[2];
                    let sebi  = nums[3]; let stamp = nums[4]; let oth = nums[5];
                    if stt   > 0.001 { charges.push(ParsedCharge { charge_type: "STT".into(),              amount_paise: to_paise(stt)   }); }
                    if exch  > 0.001 { charges.push(ParsedCharge { charge_type: "EXCHANGE_CHARGES".into(), amount_paise: to_paise(exch)  }); }
                    if sebi  > 0.001 { charges.push(ParsedCharge { charge_type: "SEBI_FEES".into(),        amount_paise: to_paise(sebi)  }); }
                    if stamp > 0.001 { charges.push(ParsedCharge { charge_type: "STAMP_DUTY".into(),       amount_paise: to_paise(stamp) }); }
                    if oth   > 0.001 { charges.push(ParsedCharge { charge_type: "OTHER".into(),            amount_paise: to_paise(oth)   }); }
                }
                _ => {} // unexpected count — skip
            }
            product_done = true;
        }

        // ── CGST/SGST "Total (Net)" row ───────────────────────────────────────
        // Format: "Total (Net)  B/f_total  CGST  SGST  IGST  UTT  Net  Taxable" (7 values)
        if in_gst_tbl && line.trim().starts_with("Total (Net)") {
            let nums = numbers_in_line(line);
            if nums.len() >= 5 {
                // [BfTotal, CGST, SGST, IGST, UTT, NetAmt, TaxableVal]
                let cgst = nums[1]; let sgst = nums[2]; let igst = nums[3];
                if cgst > 0.001 { charges.push(ParsedCharge { charge_type: "CGST".into(), amount_paise: to_paise(cgst) }); }
                if sgst > 0.001 { charges.push(ParsedCharge { charge_type: "SGST".into(), amount_paise: to_paise(sgst) }); }
                if igst > 0.001 { charges.push(ParsedCharge { charge_type: "IGST".into(), amount_paise: to_paise(igst) }); }
            }
        }
    }

    charges
}

// ─── Parse command ────────────────────────────────────────────────────────────

#[tauri::command]
pub fn parse_cn_nirmal_bang_pdf(
    file_paths: Vec<String>,
    password:   String,
) -> Result<Vec<NBCnParseResult>, String> {
    let mut results = Vec::new();

    for file_path in &file_paths {
        let text = extract_text(file_path, &password)?;
        let pages = text.split('\x0c').count();

        let (cn_number, trade_date) = parse_metadata(&text);
        let trades  = parse_trades(&text);
        let charges = parse_charges(&text);

        results.push(NBCnParseResult {
            trade_date,
            cn_number,
            client_code: None,
            trades,
            charges,
            pages_scanned: pages,
            _file_path: file_path.clone(),
        });
    }

    Ok(results)
}

// ─── Import command ───────────────────────────────────────────────────────────

#[tauri::command]
pub fn import_cn_nirmal_bang_trades(
    app:        tauri::AppHandle,
    account_id: i64,
    trade_date: String,
    cn_number:  String,
    trades:     Vec<ParsedNBTrade>,
    charges:    Vec<ParsedCharge>,
    file_paths: Option<Vec<String>>,
) -> Result<NBCnImportResult, String> {
    let conn = db::acquire()?;
    let mut imported     = 0usize;
    let mut skipped      = 0usize;
    let mut auto_created = 0usize;

    let ref_no = if cn_number.is_empty() || cn_number == "0" {
        file_paths.as_ref()
            .and_then(|ps| ps.first())
            .and_then(|p| std::path::Path::new(p).file_name())
            .map(|n| n.to_string_lossy().into_owned())
            .unwrap_or_else(|| format!("CN-NB-{}", chrono::Utc::now().timestamp()))
    } else {
        format!("CN-NB-{cn_number}")
    };

    let existing_batch_id: Option<i64> = conn
        .query_row(
            "SELECT batch_id FROM import_batches
             WHERE account_id=?1 AND source_type='CN_NIRMAL_BANG' AND ref_no=?2",
            rusqlite::params![account_id, ref_no],
            |r| r.get(0),
        )
        .ok();
    let newly_created = existing_batch_id.is_none();

    let batch_id = if let Some(id) = existing_batch_id {
        id
    } else {
        conn.execute(
            "INSERT INTO import_batches
                (account_id, source_type, ref_no, broker, batch_trade_date)
             VALUES (?1, 'CN_NIRMAL_BANG', ?2, 'Nirmal Bang Securities', ?3)",
            rusqlite::params![account_id, ref_no, trade_date],
        )
        .map_err(|e| e.to_string())?;
        conn.last_insert_rowid()
    };

    for trade in &trades {
        let (instrument_id, pending_instrument_id) = match common::resolve_equity(
            &conn,
            &trade.security_name,
            Some(&trade.isin),
            None,
            None,
            None,
            &mut auto_created,
        ) {
            Some(pair) => pair,
            None => {
                skipped += 1;
                continue;
            }
        };

        let broker_ref  = format!("{}-{}-{}", ref_no, trade.isin, trade.buy_sell);
        let price_paise = (trade.price * 100.0).round() as i64;
        let gross_paise = (trade.quantity * trade.price * 100.0).round() as i64;
        let total_value = if trade.buy_sell == "BUY" { -gross_paise } else { gross_paise };

        let rows = conn
            .execute(
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
            )
            .map_err(|e| e.to_string())?;

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

    Ok(NBCnImportResult { imported, skipped, auto_created_instruments: auto_created })
}

// ─── Tests ────────────────────────────────────────────────────────────────────

#[cfg(test)]
mod tests {
    use super::*;

    const NB_PDF: &str =
        r"C:\Users\SUMIT\OneDrive\Desktop\transaction history\18.11.2025-NIRMAL BANG.pdf";
    const PASSWORD: &str = "AAMPS4370D";

    fn skip_if_missing(path: &str) -> bool {
        if !std::path::Path::new(path).exists() {
            println!("SKIP: {path} not found");
            true
        } else {
            false
        }
    }

    #[test]
    fn test_parse_nirmal_bang_cn() {
        if skip_if_missing(NB_PDF) { return; }
        let results = parse_cn_nirmal_bang_pdf(
            vec![NB_PDF.to_string()],
            PASSWORD.to_string(),
        ).expect("parse failed");

        let r = &results[0];
        println!("CN#: {}  Date: {}  Pages: {}",
            r.cn_number, r.trade_date, r.pages_scanned);
        println!("Trades ({}):", r.trades.len());
        for t in &r.trades {
            println!("  {:4} {}  qty={:>8.0}  @ {:>10.4}  {}",
                t.buy_sell, t.isin, t.quantity, t.price, t.security_name);
        }
        println!("Charges ({}):", r.charges.len());
        for c in &r.charges {
            println!("  {:25} = {:>10.2}", c.charge_type, c.amount_paise as f64 / 100.0);
        }

        assert!(!r.trade_date.is_empty(), "trade_date empty");
        assert!(!r.cn_number.is_empty(),  "cn_number empty");
        assert!(!r.trades.is_empty(),     "no trades parsed");

        let sell = r.trades.iter().find(|t| t.buy_sell == "SELL").expect("no SELL");
        assert_eq!(sell.isin, "INE609A01010");
        assert!((sell.quantity - 4780.0).abs() < 1.0, "qty mismatch: {}", sell.quantity);
        assert!((sell.price   - 70.1329).abs() < 0.01, "price mismatch: {}", sell.price);
    }

    #[test]
    fn test_raw_pdf_header() {
        if skip_if_missing(NB_PDF) { return; }
        let bytes = std::fs::read(NB_PDF).expect("read file");
        let header = String::from_utf8_lossy(&bytes[..8.min(bytes.len())]);
        println!("PDF Header: {header:?}");
        println!("File size: {} bytes", bytes.len());
        let has_xref_stream = bytes.windows(5).any(|w| w == b"/XRef");
        println!("Has /XRef stream: {has_xref_stream}");
    }
}
