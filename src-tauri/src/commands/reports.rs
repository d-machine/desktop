use crate::db;
use rust_xlsxwriter::{Format, FormatAlign, FormatBorder, Workbook, Color};
use serde::Serialize;
use std::collections::HashMap;

/// One matched lot: a portion of a buy consumed by a specific sell.
#[derive(Serialize, Clone)]
pub struct CapitalGainLot {
    pub instrument_id: i64,
    pub instrument_name: String,
    pub isin: Option<String>,
    pub asset_class: String,
    pub tax_category: String,
    pub account_name: String,
    pub buy_date: String,
    pub sell_date: String,
    pub quantity: f64,
    pub buy_price_paise: i64,   // cost per unit (incl. buy charges)
    pub sell_price_paise: i64,  // proceeds per unit (net sell charges)
    pub cost_paise: i64,        // buy_price * qty (rounded)
    pub proceeds_paise: i64,    // sell_price * qty (rounded)
    pub gain_paise: i64,        // proceeds - cost
    pub holding_days: i64,
    pub gain_type: String,      // "STCG","LTCG","SPECULATIVE","NON_SPECULATIVE"
    pub fy: String,             // e.g. "2024-25"
}

#[derive(Serialize, Clone)]
pub struct CapitalGainsSummary {
    pub fy: String,
    pub stcg_equity_paise: i64,
    pub ltcg_equity_paise: i64,
    pub stcg_debt_paise: i64,
    pub ltcg_debt_paise: i64,
    pub speculative_paise: i64,
    pub non_speculative_paise: i64,
    pub total_gain_paise: i64,
}

#[derive(Serialize)]
pub struct CapitalGainsReport {
    pub lots: Vec<CapitalGainLot>,
    pub summaries: Vec<CapitalGainsSummary>,
    pub all_fys: Vec<String>,
}

// Internal buy lot, consumed by FIFO
struct BuyLot {
    trade_date: String,
    cost_per_unit_paise: i64,  // abs(total_value) / qty — includes all charges
    remaining_qty: f64,
}

// Row loaded from DB
struct TxnRow {
    account_id: i64,
    account_name: String,
    instrument_id: i64,
    instrument_name: String,
    isin: Option<String>,
    asset_class: String,
    tax_category: String,
    trade_date: String,
    txn_type: String,
    quantity: f64,
    total_value_paise: i64,
    price_paise: i64,
}

/// Indian financial year containing a given ISO date (YYYY-MM-DD).
/// April → March. Returns e.g. "2024-25".
fn fy_of(date: &str) -> String {
    let year: i32 = date.get(0..4).and_then(|s| s.parse().ok()).unwrap_or(2024);
    let month: u32 = date.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(1);
    if month >= 4 {
        format!("{}-{:02}", year, (year + 1) % 100)
    } else {
        format!("{}-{:02}", year - 1, year % 100)
    }
}

/// Days between two ISO date strings.
fn holding_days(buy: &str, sell: &str) -> i64 {
    fn to_days(d: &str) -> i64 {
        let y: i64 = d.get(0..4).and_then(|s| s.parse().ok()).unwrap_or(2000);
        let m: i64 = d.get(5..7).and_then(|s| s.parse().ok()).unwrap_or(1);
        let day: i64 = d.get(8..10).and_then(|s| s.parse().ok()).unwrap_or(1);
        // Rough Julian day number (accurate for comparison purposes)
        let a = (14 - m) / 12;
        let yr = y + 4800 - a;
        let mn = m + 12 * a - 3;
        day + (153 * mn + 2) / 5 + 365 * yr + yr / 4 - yr / 100 + yr / 400 - 32045
    }
    to_days(sell) - to_days(buy)
}

/// Classify a closed lot into one of four gain type buckets.
fn gain_type(tax_category: &str, trade_segment: &str, days: i64) -> String {
    // Intraday equity → speculative
    if trade_segment == "INTRADAY" {
        return "SPECULATIVE".to_string();
    }
    match tax_category {
        "EQUITY_LTCG" => {
            if days >= 365 { "LTCG".to_string() } else { "STCG".to_string() }
        }
        "DEBT" => {
            // Pre-2023 debt MF: 3 years for LTCG. Post-Apr 2023 amendment: always STCG.
            // We apply the conservative rule: < 1095 days = STCG.
            if days >= 1095 { "LTCG".to_string() } else { "STCG".to_string() }
        }
        "NON_SPECULATIVE" | "SPECULATIVE" => "NON_SPECULATIVE".to_string(),
        _ => {
            if days >= 365 { "LTCG".to_string() } else { "STCG".to_string() }
        }
    }
}

#[tauri::command]
pub fn get_capital_gains(
    fy: Option<String>,
    account_ids: Option<Vec<i64>>,
) -> Result<CapitalGainsReport, String> {
    let conn = db::acquire()?;

    // Build account filter
    let acct_sql = match &account_ids {
        Some(ids) if !ids.is_empty() => {
            let ph = ids.iter().enumerate()
                .map(|(i, _)| format!("?{}", i + 1))
                .collect::<Vec<_>>().join(",");
            format!("AND t.account_id IN ({})", ph)
        }
        _ => String::new(),
    };

    // For pending instruments we derive asset_class / tax_category from pi.type.
    // EQUITY/INDEX → EQUITY_LTCG; MF variants → EQUITY_LTCG (conservative);
    // FNO variants → NON_SPECULATIVE; MCX → NON_SPECULATIVE.
    let sql = format!(
        "SELECT t.account_id,
                a.name,
                COALESCE(t.instrument_id, -t.pending_instrument_id) AS instrument_id,
                COALESCE(i.name, pi.name)                           AS instrument_name,
                COALESCE(ie.isin, json_extract(pi.metadata, '$.isin')) AS isin,
                COALESCE(it.asset_class,
                    CASE pi.type
                        WHEN 'EQUITY'  THEN 'EQUITY'
                        WHEN 'MF'      THEN 'MF'
                        WHEN 'FUTSTK'  THEN 'DERIVATIVE'
                        WHEN 'FUTIDX'  THEN 'DERIVATIVE'
                        WHEN 'OPTSTK'  THEN 'DERIVATIVE'
                        WHEN 'OPTIDX'  THEN 'DERIVATIVE'
                        WHEN 'MCX'     THEN 'COMMODITY'
                        ELSE 'EQUITY'
                    END
                ) AS asset_class,
                COALESCE(it.tax_category,
                    CASE pi.type
                        WHEN 'EQUITY'  THEN 'EQUITY_LTCG'
                        WHEN 'MF'      THEN 'EQUITY_LTCG'
                        WHEN 'FUTSTK'  THEN 'NON_SPECULATIVE'
                        WHEN 'FUTIDX'  THEN 'NON_SPECULATIVE'
                        WHEN 'OPTSTK'  THEN 'NON_SPECULATIVE'
                        WHEN 'OPTIDX'  THEN 'NON_SPECULATIVE'
                        WHEN 'MCX'     THEN 'NON_SPECULATIVE'
                        ELSE 'EQUITY_LTCG'
                    END
                ) AS tax_category,
                t.trade_date, t.txn_type,
                t.quantity, t.total_value_paise, t.price_paise
         FROM transactions t
         JOIN accounts a ON t.account_id = a.account_id
         LEFT JOIN instruments i ON i.instrument_id = t.instrument_id
         LEFT JOIN instrument_types it ON it.instrument_type_id = i.instrument_type_id
         LEFT JOIN instrument_equity ie ON ie.instrument_id = i.instrument_id
         LEFT JOIN pending_instruments pi ON pi.pending_id = t.pending_instrument_id
         WHERE t.txn_type IN (
             'BUY','SIP','IPO','FPO','OPENING_BALANCE','BONUS',
             'MERGER_IN','SWITCH_IN','TRANSFER_IN','SPLIT_IN',
             'SELL','REDEMPTION','MERGER_OUT','SWITCH_OUT','TRANSFER_OUT','SPLIT_OUT'
         )
         {acct_sql}
         ORDER BY t.account_id,
                  COALESCE(t.instrument_id, -t.pending_instrument_id),
                  t.trade_date ASC, t.txn_id ASC"
    );

    let acct_params: Vec<Box<dyn rusqlite::ToSql>> = match &account_ids {
        Some(ids) if !ids.is_empty() => ids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>).collect(),
        _ => vec![],
    };

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows: Vec<TxnRow> = stmt.query_map(
        rusqlite::params_from_iter(acct_params.iter().map(|p| p.as_ref())),
        |row| Ok(TxnRow {
            account_id: row.get(0)?,
            account_name: row.get(1)?,
            instrument_id: row.get(2)?,
            instrument_name: row.get(3)?,
            isin: row.get(4)?,
            asset_class: row.get(5)?,
            tax_category: row.get(6)?,
            trade_date: row.get(7)?,
            txn_type: row.get(8)?,
            quantity: row.get(9)?,
            total_value_paise: row.get(10)?,
            price_paise: row.get(11)?,
        }),
    )
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    // Cost-basis transfers (SPLIT, TRANSFER) are not taxable events.
    // They drain/seed the FIFO queue but don't produce capital gain lots.
    let is_buy = |t: &str| matches!(
        t, "BUY" | "SIP" | "IPO" | "FPO" | "OPENING_BALANCE" | "BONUS"
         | "MERGER_IN" | "SWITCH_IN" | "TRANSFER_IN" | "SPLIT_IN"
    );
    let is_taxable_sell = |t: &str| matches!(
        t, "SELL" | "REDEMPTION" | "MERGER_OUT" | "SWITCH_OUT"
    );
    let is_silent_sell = |t: &str| matches!(t, "TRANSFER_OUT" | "SPLIT_OUT");

    // Tradeable types eligible for same-day netting across all asset classes.
    // Corporate actions (BONUS, SPLIT, MERGER, TRANSFER, OPENING_BALANCE) are not
    // intraday trades and are excluded — they go straight into the FIFO.
    let is_trading_buy  = |t: &str| matches!(t, "BUY" | "IPO" | "FPO" | "SIP");
    let is_trading_sell = |t: &str| matches!(t, "SELL" | "REDEMPTION");

    // Same-day gain type by asset class:
    //   Equity intraday → SPECULATIVE (Section 43(5))
    //   F&O / MCX intraday → NON_SPECULATIVE (F&O is always non-speculative)
    //   MF same-day → STCG (holding period = 0, rate follows short-term rules)
    let same_day_gain_type = |asset_class: &str, tax_category: &str| -> &'static str {
        match (asset_class, tax_category) {
            ("EQUITY", _) => "SPECULATIVE",
            (_, "NON_SPECULATIVE") => "NON_SPECULATIVE",
            _ => "STCG",
        }
    };

    // ── Phase 1: Day-level totals for same-day netting (all asset classes) ───
    // key = (account_id, instrument_id, trade_date)
    struct DayTotals {
        buy_qty: f64, buy_value_paise: f64,
        sell_qty: f64, sell_value_paise: f64,
        instrument_id: i64,
        asset_class: String,
        account_name: String, instrument_name: String,
        isin: Option<String>, tax_category: String,
    }
    let mut day_map: HashMap<(i64, i64, String), DayTotals> = HashMap::new();

    for row in &rows {
        if !is_trading_buy(&row.txn_type) && !is_trading_sell(&row.txn_type) { continue; }

        let key = (row.account_id, row.instrument_id, row.trade_date.clone());
        let entry = day_map.entry(key).or_insert_with(|| DayTotals {
            buy_qty: 0.0, buy_value_paise: 0.0,
            sell_qty: 0.0, sell_value_paise: 0.0,
            instrument_id: row.instrument_id,
            asset_class: row.asset_class.clone(),
            account_name: row.account_name.clone(),
            instrument_name: row.instrument_name.clone(),
            isin: row.isin.clone(), tax_category: row.tax_category.clone(),
        });

        if is_trading_buy(&row.txn_type) {
            entry.buy_qty          += row.quantity;
            entry.buy_value_paise  += row.total_value_paise.unsigned_abs() as f64;
        } else {
            entry.sell_qty         += row.quantity;
            entry.sell_value_paise += row.total_value_paise.unsigned_abs() as f64;
        }
    }

    let mut lots: Vec<CapitalGainLot> = Vec::new();

    // ── Phase 2: Same-day matched lots ───────────────────────────────────────
    let mut day_keys: Vec<(i64, i64, String)> = day_map.keys().cloned().collect();
    day_keys.sort();

    for key in &day_keys {
        let t = &day_map[key];
        let matched_qty = t.buy_qty.min(t.sell_qty);
        if matched_qty < 0.0001 { continue; }

        let avg_buy  = t.buy_value_paise  / t.buy_qty;
        let avg_sell = t.sell_value_paise / t.sell_qty;
        let cost     = (avg_buy  * matched_qty) as i64;
        let proceeds = (avg_sell * matched_qty) as i64;
        let gt       = same_day_gain_type(&t.asset_class, &t.tax_category).to_string();

        lots.push(CapitalGainLot {
            instrument_id: t.instrument_id, instrument_name: t.instrument_name.clone(),
            isin: t.isin.clone(), asset_class: t.asset_class.clone(),
            tax_category: t.tax_category.clone(), account_name: t.account_name.clone(),
            buy_date: key.2.clone(), sell_date: key.2.clone(),
            quantity: matched_qty,
            buy_price_paise: avg_buy as i64, sell_price_paise: avg_sell as i64,
            cost_paise: cost, proceeds_paise: proceeds, gain_paise: proceeds - cost,
            holding_days: 0, gain_type: gt, fy: fy_of(&key.2),
        });
    }

    // ── Phase 3: Delivery FIFO on net quantities ──────────────────────────────
    // Each day's same-day matched budget is pre-computed in Phase 1.
    // As rows are processed, the matched portion is absorbed first;
    // only the residual delivery quantity enters the FIFO queue.
    struct IntraBudget { buy_rem: f64, sell_rem: f64 }
    let mut intra: HashMap<(i64, i64, String), IntraBudget> = HashMap::new();
    for key in &day_keys {
        let matched_qty = { let t = &day_map[key]; t.buy_qty.min(t.sell_qty) };
        if matched_qty > 0.0001 {
            intra.insert(key.clone(), IntraBudget { buy_rem: matched_qty, sell_rem: matched_qty });
        }
    }

    let mut buy_queues: HashMap<(i64, i64), Vec<BuyLot>> = HashMap::new();

    for row in &rows {
        let key = (row.account_id, row.instrument_id);

        if is_buy(&row.txn_type) {
            let delivery_qty = if is_trading_buy(&row.txn_type) {
                let bkey = (row.account_id, row.instrument_id, row.trade_date.clone());
                let used = intra.get_mut(&bkey)
                    .map(|b| { let d = row.quantity.min(b.buy_rem); b.buy_rem -= d; d })
                    .unwrap_or(0.0);
                row.quantity - used
            } else {
                row.quantity
            };
            if delivery_qty < 0.0001 { continue; }

            let cost_per_unit = if row.quantity > 0.0 {
                (row.total_value_paise.unsigned_abs() as f64 / row.quantity) as i64
            } else {
                row.price_paise
            };
            buy_queues.entry(key).or_default().push(BuyLot {
                trade_date: row.trade_date.clone(),
                cost_per_unit_paise: cost_per_unit,
                remaining_qty: delivery_qty,
            });

        } else if is_taxable_sell(&row.txn_type) || is_silent_sell(&row.txn_type) {
            let taxable = is_taxable_sell(&row.txn_type);

            let delivery_qty = if is_trading_sell(&row.txn_type) {
                let bkey = (row.account_id, row.instrument_id, row.trade_date.clone());
                let used = intra.get_mut(&bkey)
                    .map(|b| { let d = row.quantity.min(b.sell_rem); b.sell_rem -= d; d })
                    .unwrap_or(0.0);
                row.quantity - used
            } else {
                row.quantity
            };
            if delivery_qty < 0.0001 { continue; }

            let sell_proceeds_per_unit = if row.quantity > 0.0 {
                (row.total_value_paise.unsigned_abs() as f64 / row.quantity) as i64
            } else {
                row.price_paise
            };

            let queue = buy_queues.entry(key).or_default();
            let mut qty_to_match = delivery_qty;

            for lot in queue.iter_mut() {
                if qty_to_match <= 0.0001 { break; }
                if lot.remaining_qty <= 0.0001 { continue; }

                let matched = qty_to_match.min(lot.remaining_qty);
                lot.remaining_qty -= matched;
                qty_to_match -= matched;

                if !taxable { continue; }

                let cost    = (lot.cost_per_unit_paise as f64 * matched) as i64;
                let proceeds = (sell_proceeds_per_unit  as f64 * matched) as i64;
                let days = holding_days(&lot.trade_date, &row.trade_date);
                let gt   = gain_type(&row.tax_category, "DELIVERY", days);

                lots.push(CapitalGainLot {
                    instrument_id: row.instrument_id, instrument_name: row.instrument_name.clone(),
                    isin: row.isin.clone(), asset_class: row.asset_class.clone(),
                    tax_category: row.tax_category.clone(), account_name: row.account_name.clone(),
                    buy_date: lot.trade_date.clone(), sell_date: row.trade_date.clone(),
                    quantity: matched,
                    buy_price_paise: lot.cost_per_unit_paise, sell_price_paise: sell_proceeds_per_unit,
                    cost_paise: cost, proceeds_paise: proceeds, gain_paise: proceeds - cost,
                    holding_days: days, gain_type: gt, fy: fy_of(&row.trade_date),
                });
            }
        }
    }

    // Sort lots newest sell first
    lots.sort_by(|a, b| b.sell_date.cmp(&a.sell_date));

    // Collect all FYs present
    let mut all_fys: Vec<String> = lots.iter().map(|l| l.fy.clone()).collect::<std::collections::HashSet<_>>().into_iter().collect();
    all_fys.sort_by(|a, b| b.cmp(a)); // newest first

    // Build per-FY summaries
    let fys_to_summarize: Vec<String> = if all_fys.is_empty() {
        vec![fy_of(&chrono_today())]
    } else {
        all_fys.clone()
    };

    let filtered_lots: Vec<CapitalGainLot> = match &fy {
        Some(f) => lots.iter().filter(|l| &l.fy == f).cloned().collect(),
        None => lots.clone(),
    };

    let summaries: Vec<CapitalGainsSummary> = fys_to_summarize.iter().map(|f| {
        let fy_lots: Vec<&CapitalGainLot> = lots.iter().filter(|l| &l.fy == f).collect();
        let mut stcg_equity = 0i64;
        let mut ltcg_equity = 0i64;
        let mut stcg_debt = 0i64;
        let mut ltcg_debt = 0i64;
        let mut speculative = 0i64;
        let mut non_spec = 0i64;

        for lot in &fy_lots {
            match lot.gain_type.as_str() {
                "STCG" => {
                    if lot.tax_category == "DEBT" { stcg_debt += lot.gain_paise; }
                    else { stcg_equity += lot.gain_paise; }
                }
                "LTCG" => {
                    if lot.tax_category == "DEBT" { ltcg_debt += lot.gain_paise; }
                    else { ltcg_equity += lot.gain_paise; }
                }
                "SPECULATIVE" => { speculative += lot.gain_paise; }
                "NON_SPECULATIVE" => { non_spec += lot.gain_paise; }
                _ => { stcg_equity += lot.gain_paise; }
            }
        }

        let total = stcg_equity + ltcg_equity + stcg_debt + ltcg_debt + speculative + non_spec;
        CapitalGainsSummary {
            fy: f.clone(),
            stcg_equity_paise: stcg_equity,
            ltcg_equity_paise: ltcg_equity,
            stcg_debt_paise: stcg_debt,
            ltcg_debt_paise: ltcg_debt,
            speculative_paise: speculative,
            non_speculative_paise: non_spec,
            total_gain_paise: total,
        }
    }).collect();

    Ok(CapitalGainsReport {
        lots: filtered_lots,
        summaries,
        all_fys,
    })
}

// ─────────────────────────────────────────────────────────────────────────────
// Income Report
// ─────────────────────────────────────────────────────────────────────────────

#[derive(Serialize, Clone)]
pub struct IncomeEvent {
    pub txn_id: i64,
    pub instrument_id: i64,
    pub instrument_name: String,
    pub isin: Option<String>,
    pub asset_class: String,
    pub account_name: String,
    pub income_type: String,   // "DIVIDEND" | "INTEREST"
    pub trade_date: String,
    pub amount_paise: i64,     // gross income
    pub notes: Option<String>,
    pub fy: String,
}

#[derive(Serialize)]
pub struct IncomeSummary {
    pub fy: String,
    pub dividend_paise: i64,
    pub interest_paise: i64,
    pub total_paise: i64,
}

#[derive(Serialize)]
pub struct IncomeReport {
    pub events: Vec<IncomeEvent>,
    pub summaries: Vec<IncomeSummary>,
    pub all_fys: Vec<String>,
}

#[tauri::command]
pub fn get_income(
    fy: Option<String>,
    account_ids: Option<Vec<i64>>,
) -> Result<IncomeReport, String> {
    let conn = db::acquire()?;

    let acct_sql = match &account_ids {
        Some(ids) if !ids.is_empty() => {
            let ph = ids.iter().enumerate()
                .map(|(i, _)| format!("?{}", i + 1))
                .collect::<Vec<_>>().join(",");
            format!("AND t.account_id IN ({})", ph)
        }
        _ => String::new(),
    };

    let sql = format!(
        "SELECT t.txn_id, t.instrument_id, i.name, ie.isin,
                it.asset_class, a.name AS account_name,
                t.txn_type, t.trade_date, t.total_value_paise, t.notes
         FROM transactions t
         JOIN accounts a ON t.account_id = a.account_id
         JOIN instruments i ON t.instrument_id = i.instrument_id
         JOIN instrument_types it ON i.instrument_type_id = it.instrument_type_id
         LEFT JOIN instrument_equity ie ON ie.instrument_id = i.instrument_id
         WHERE t.txn_type IN ('DIVIDEND', 'INTEREST') {acct_sql}
         ORDER BY t.trade_date DESC, t.txn_id DESC"
    );

    let acct_params: Vec<Box<dyn rusqlite::ToSql>> = match &account_ids {
        Some(ids) if !ids.is_empty() => ids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>).collect(),
        _ => vec![],
    };

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let events: Vec<IncomeEvent> = stmt.query_map(
        rusqlite::params_from_iter(acct_params.iter().map(|p| p.as_ref())),
        |row| {
            let trade_date: String = row.get(7)?;
            let fy = fy_of(&trade_date);
            Ok(IncomeEvent {
                txn_id: row.get(0)?,
                instrument_id: row.get(1)?,
                instrument_name: row.get(2)?,
                isin: row.get(3)?,
                asset_class: row.get(4)?,
                account_name: row.get(5)?,
                income_type: row.get(6)?,
                trade_date,
                amount_paise: row.get::<_, i64>(8)?.abs(),
                notes: row.get(9)?,
                fy,
            })
        },
    )
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    // Collect FYs
    let mut all_fys: Vec<String> = events.iter().map(|e| e.fy.clone())
        .collect::<std::collections::HashSet<_>>().into_iter().collect();
    all_fys.sort_by(|a, b| b.cmp(a));

    // Per-FY summaries
    let summaries: Vec<IncomeSummary> = all_fys.iter().map(|f| {
        let div: i64 = events.iter().filter(|e| &e.fy == f && e.income_type == "DIVIDEND").map(|e| e.amount_paise).sum();
        let int: i64 = events.iter().filter(|e| &e.fy == f && e.income_type == "INTEREST").map(|e| e.amount_paise).sum();
        IncomeSummary { fy: f.clone(), dividend_paise: div, interest_paise: int, total_paise: div + int }
    }).collect();

    let filtered: Vec<IncomeEvent> = match &fy {
        Some(f) => events.into_iter().filter(|e| &e.fy == f).collect(),
        None => events,
    };

    Ok(IncomeReport { events: filtered, summaries, all_fys })
}

// ─────────────────────────────────────────────────────────────────────────────
// Excel Tax Report Export
// ─────────────────────────────────────────────────────────────────────────────

/// Export a full tax report for the given FY to an Excel file at `path`.
/// Sheets: (1) CG Summary, (2) Capital Gains Lots, (3) Income.
#[tauri::command]
pub fn export_tax_report(fy: String, path: String) -> Result<(), String> {
    // Fetch data
    let cg = get_capital_gains(Some(fy.clone()), None)?;
    let income = get_income(Some(fy.clone()), None)?;
    let cg_summary = cg.summaries.iter().find(|s| s.fy == fy).cloned()
        .unwrap_or(CapitalGainsSummary {
            fy: fy.clone(),
            stcg_equity_paise: 0, ltcg_equity_paise: 0,
            stcg_debt_paise: 0, ltcg_debt_paise: 0,
            speculative_paise: 0, non_speculative_paise: 0,
            total_gain_paise: 0,
        });

    // ── Formats ──────────────────────────────────────────────────────────────
    let hdr = Format::new()
        .set_bold()
        .set_background_color(Color::RGB(0x1e293b))
        .set_font_color(Color::RGB(0xf8fafc))
        .set_border(FormatBorder::Thin)
        .set_align(FormatAlign::Center);

    let label = Format::new()
        .set_bold()
        .set_background_color(Color::RGB(0xf1f5f9))
        .set_border(FormatBorder::Thin);

    let money = Format::new()
        .set_num_format("₹#,##0.00")
        .set_border(FormatBorder::Thin);

    let money_pos = Format::new()
        .set_num_format("₹#,##0.00")
        .set_font_color(Color::RGB(0x16a34a))
        .set_bold()
        .set_border(FormatBorder::Thin);

    let money_neg = Format::new()
        .set_num_format("₹#,##0.00")
        .set_font_color(Color::RGB(0xdc2626))
        .set_bold()
        .set_border(FormatBorder::Thin);

    let cell = Format::new().set_border(FormatBorder::Thin);
    let date_fmt = Format::new().set_num_format("DD-MMM-YYYY").set_border(FormatBorder::Thin);
    let num_fmt = Format::new().set_num_format("#,##0.####").set_border(FormatBorder::Thin);

    let paise_to_rupees = |p: i64| p as f64 / 100.0;

    let mut wb = Workbook::new();

    // ── Sheet 1: Capital Gains Summary ───────────────────────────────────────
    {
        let ws = wb.add_worksheet();
        ws.set_name("CG Summary").map_err(|e| e.to_string())?;
        ws.set_column_width(0, 30).map_err(|e| e.to_string())?;
        ws.set_column_width(1, 18).map_err(|e| e.to_string())?;

        ws.write_with_format(0, 0, format!("Capital Gains Summary – FY {fy}"), &Format::new().set_bold().set_font_size(13))
            .map_err(|e| e.to_string())?;

        let rows: &[(&str, i64)] = &[
            ("STCG – Equity",        cg_summary.stcg_equity_paise),
            ("LTCG – Equity",        cg_summary.ltcg_equity_paise),
            ("STCG – Debt",          cg_summary.stcg_debt_paise),
            ("LTCG – Debt",          cg_summary.ltcg_debt_paise),
            ("Speculative Gains",    cg_summary.speculative_paise),
            ("Non-Speculative Gains",cg_summary.non_speculative_paise),
            ("Total Realized Gain",  cg_summary.total_gain_paise),
        ];

        for (i, (lbl, paise)) in rows.iter().enumerate() {
            let r = (i + 2) as u32;
            let rupees = paise_to_rupees(*paise);
            let fmt = if *lbl == "Total Realized Gain" {
                if rupees >= 0.0 { &money_pos } else { &money_neg }
            } else { &money };
            ws.write_with_format(r, 0, *lbl, &label).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 1, rupees, fmt).map_err(|e| e.to_string())?;
        }
    }

    // ── Sheet 2: Capital Gains Lots ───────────────────────────────────────────
    {
        let ws = wb.add_worksheet();
        ws.set_name("CG Details").map_err(|e| e.to_string())?;

        let headers = ["Instrument", "ISIN", "Account", "Type", "Asset Class",
                       "Buy Date", "Sell Date", "Hold Days", "Qty",
                       "Buy Price (₹)", "Sell Price (₹)", "Cost (₹)", "Proceeds (₹)", "Gain (₹)", "Gain Type"];
        let widths = [28.0, 14.0, 18.0, 14.0, 14.0, 13.0, 13.0, 10.0, 10.0, 14.0, 14.0, 14.0, 14.0, 14.0, 16.0];
        for (i, (h, w)) in headers.iter().zip(widths.iter()).enumerate() {
            ws.set_column_width(i as u16, *w).map_err(|e| e.to_string())?;
            ws.write_with_format(0, i as u16, *h, &hdr).map_err(|e| e.to_string())?;
        }
        ws.set_row_height(0, 18.0).map_err(|e| e.to_string())?;

        for (i, lot) in cg.lots.iter().enumerate() {
            let r = (i + 1) as u32;
            let gain_fmt = if lot.gain_paise >= 0 { &money_pos } else { &money_neg };
            ws.write_with_format(r, 0,  &lot.instrument_name, &cell).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 1,  lot.isin.as_deref().unwrap_or(""), &cell).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 2,  &lot.account_name, &cell).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 3,  &lot.tax_category, &cell).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 4,  &lot.asset_class, &cell).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 5,  &lot.buy_date, &date_fmt).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 6,  &lot.sell_date, &date_fmt).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 7,  lot.holding_days, &cell).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 8,  lot.quantity, &num_fmt).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 9,  paise_to_rupees(lot.buy_price_paise), &money).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 10, paise_to_rupees(lot.sell_price_paise), &money).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 11, paise_to_rupees(lot.cost_paise), &money).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 12, paise_to_rupees(lot.proceeds_paise), &money).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 13, paise_to_rupees(lot.gain_paise), gain_fmt).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 14, &lot.gain_type, &cell).map_err(|e| e.to_string())?;
        }
    }

    // ── Sheet 3: Income ───────────────────────────────────────────────────────
    {
        let ws = wb.add_worksheet();
        ws.set_name("Income").map_err(|e| e.to_string())?;

        let headers = ["Date", "Instrument", "ISIN", "Account", "Type", "Amount (₹)", "Notes"];
        let widths = [13.0, 28.0, 14.0, 18.0, 12.0, 14.0, 30.0];
        for (i, (h, w)) in headers.iter().zip(widths.iter()).enumerate() {
            ws.set_column_width(i as u16, *w).map_err(|e| e.to_string())?;
            ws.write_with_format(0, i as u16, *h, &hdr).map_err(|e| e.to_string())?;
        }
        ws.set_row_height(0, 18.0).map_err(|e| e.to_string())?;

        for (i, ev) in income.events.iter().enumerate() {
            let r = (i + 1) as u32;
            ws.write_with_format(r, 0, &ev.trade_date, &date_fmt).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 1, &ev.instrument_name, &cell).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 2, ev.isin.as_deref().unwrap_or(""), &cell).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 3, &ev.account_name, &cell).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 4, &ev.income_type, &cell).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 5, paise_to_rupees(ev.amount_paise), &money_pos).map_err(|e| e.to_string())?;
            ws.write_with_format(r, 6, ev.notes.as_deref().unwrap_or(""), &cell).map_err(|e| e.to_string())?;
        }
    }

    wb.save(&path).map_err(|e| e.to_string())?;
    Ok(())
}

fn chrono_today() -> String {
    // Simple approach without chrono dependency just for FY default
    // Use a fixed fallback; chrono is available in Cargo.toml
    use std::time::{SystemTime, UNIX_EPOCH};
    let secs = SystemTime::now().duration_since(UNIX_EPOCH).unwrap_or_default().as_secs();
    // Approximate: days since epoch, then to year-month
    let days = secs / 86400;
    let y400 = days / 146097;
    let remaining = days % 146097;
    let year = 1970 + y400 * 400 + remaining / 365;
    // Just return a rough current year string — good enough for FY default
    format!("{}-01-01", year)
}
