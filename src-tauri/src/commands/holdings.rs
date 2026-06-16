use crate::db;
use serde::Serialize;
use std::collections::HashMap;

#[derive(Serialize)]
pub struct Holding {
    pub instrument_id: i64,
    pub instrument_name: String,
    pub isin: Option<String>,
    pub instrument_type: String,
    pub asset_class: String,
    pub is_pending: bool,
    pub account_id: i64,
    pub account_name: String,
    pub portfolio_id: i64,
    pub quantity: f64,
    pub avg_cost_paise: i64,
    pub total_cost_paise: i64,
    pub current_price_paise: Option<i64>,
    pub current_value_paise: Option<i64>,
    pub unrealized_pnl_paise: Option<i64>,
    pub unrealized_pnl_pct: Option<f64>,
    pub price_date: Option<String>,
}

#[derive(Serialize)]
pub struct PortfolioSummary {
    pub total_invested_paise: i64,
    pub current_value_paise: Option<i64>,
    pub unrealized_pnl_paise: Option<i64>,
    pub unrealized_pnl_pct: Option<f64>,
    pub holdings_count: i64,
    pub accounts_count: i64,
}

// One row from the transaction query
struct TxnRow {
    txn_id: i64,
    account_id: i64,
    account_name: String,
    portfolio_id: i64,
    instrument_id: i64,
    instrument_name: String,
    isin: Option<String>,
    instrument_type: String,
    asset_class: String,
    is_pending: bool,
    trade_date: String,
    txn_type: String,
    quantity: f64,
    price_paise: i64,
    current_price: Option<i64>,
    price_date: Option<String>,
}

// A remaining buy lot after FIFO matching
struct BuyLot {
    price_paise: i64,
    remaining_qty: f64,
}

// Metadata that is the same for all txns of a position
struct PositionMeta {
    account_name: String,
    portfolio_id: i64,
    instrument_name: String,
    isin: Option<String>,
    instrument_type: String,
    asset_class: String,
    is_pending: bool,
    current_price: Option<i64>,
    price_date: Option<String>,
}

fn is_buy(t: &str) -> bool {
    matches!(t, "BUY" | "SIP" | "IPO" | "FPO" | "OPENING_BALANCE" | "BONUS"
              | "MERGER_IN" | "SWITCH_IN" | "TRANSFER_IN" | "SPLIT_IN")
}

fn is_sell(t: &str) -> bool {
    matches!(t, "SELL" | "REDEMPTION" | "MERGER_OUT" | "SWITCH_OUT" | "TRANSFER_OUT" | "SPLIT_OUT")
}

/// Compute current holdings using FIFO lot matching.
///
/// Intraday trades are netted per day before entering the FIFO:
///   - If intraday buys > sells on a given day, the excess becomes a delivery BUY lot.
///   - If intraday sells > buys on a given day, the excess becomes a delivery SELL.
/// This means only the imbalance of intraday activity affects the held position.
#[tauri::command]
pub fn get_holdings(
    account_ids:   Option<Vec<i64>>,
    portfolio_ids: Option<Vec<i64>>,
    asset_classes: Option<Vec<String>>,
) -> Result<Vec<Holding>, String> {
    let conn = db::acquire()?;

    // Build WHERE clauses and accumulate bound params
    let mut filters: Vec<String> = Vec::new();
    let mut params: Vec<Box<dyn rusqlite::ToSql>> = Vec::new();

    if let Some(ids) = &account_ids {
        if !ids.is_empty() {
            let ph = (1..=ids.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",");
            filters.push(format!("t.account_id IN ({ph})"));
            params.extend(ids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>));
        }
    }

    if let Some(pids) = &portfolio_ids {
        if !pids.is_empty() {
            let base = params.len() + 1;
            let ph = (base..base + pids.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",");
            filters.push(format!("a.portfolio_id IN ({ph})"));
            params.extend(pids.iter().map(|id| Box::new(*id) as Box<dyn rusqlite::ToSql>));
        }
    }

    if let Some(classes) = &asset_classes {
        if !classes.is_empty() {
            let base = params.len() + 1;
            let ph = (base..base + classes.len()).map(|i| format!("?{i}")).collect::<Vec<_>>().join(",");
            filters.push(format!(
                "COALESCE(it.asset_class, CASE pi.type \
                 WHEN 'EQUITY' THEN 'EQUITY' \
                 WHEN 'MF'     THEN 'MF' \
                 WHEN 'FUTSTK' THEN 'DERIVATIVE' \
                 WHEN 'FUTIDX' THEN 'DERIVATIVE' \
                 WHEN 'OPTSTK' THEN 'DERIVATIVE' \
                 WHEN 'OPTIDX' THEN 'DERIVATIVE' \
                 WHEN 'MCX'    THEN 'COMMODITY' \
                 ELSE 'UNKNOWN' END) IN ({ph})"
            ));
            params.extend(classes.iter().map(|c| Box::new(c.clone()) as Box<dyn rusqlite::ToSql>));
        }
    }

    let extra = if filters.is_empty() {
        String::new()
    } else {
        format!("AND {}", filters.join(" AND "))
    };

    let sql = format!(
        "SELECT t.txn_id, t.account_id, a.name, a.portfolio_id,
                COALESCE(t.instrument_id, -t.pending_instrument_id) AS instrument_id,
                COALESCE(i.name, pi.name)                           AS instrument_name,
                COALESCE(ie.isin, json_extract(pi.metadata, '$.isin')) AS isin,
                COALESCE(it.name, pi.type)                          AS instrument_type,
                COALESCE(it.asset_class, CASE pi.type
                    WHEN 'EQUITY' THEN 'EQUITY'
                    WHEN 'MF'     THEN 'MF'
                    WHEN 'FUTSTK' THEN 'DERIVATIVE'
                    WHEN 'FUTIDX' THEN 'DERIVATIVE'
                    WHEN 'OPTSTK' THEN 'DERIVATIVE'
                    WHEN 'OPTIDX' THEN 'DERIVATIVE'
                    WHEN 'MCX'    THEN 'COMMODITY'
                    ELSE 'UNKNOWN' END)                             AS asset_class,
                (t.pending_instrument_id IS NOT NULL)               AS is_pending,
                t.trade_date,
                t.txn_type, t.quantity, t.price_paise,
                lp.close_price_paise, lp.price_date
         FROM transactions t
         JOIN accounts a ON t.account_id = a.account_id
         LEFT JOIN instruments i        ON t.instrument_id      = i.instrument_id
         LEFT JOIN instrument_types it  ON i.instrument_type_id = it.instrument_type_id
         LEFT JOIN instrument_equity ie ON ie.instrument_id     = i.instrument_id
         LEFT JOIN pending_instruments pi ON pi.pending_id      = t.pending_instrument_id
         LEFT JOIN latest_prices lp     ON lp.instrument_id     = t.instrument_id
         WHERE t.txn_type IN (
             'BUY','SIP','IPO','FPO','OPENING_BALANCE','BONUS',
             'MERGER_IN','SWITCH_IN','TRANSFER_IN','SPLIT_IN',
             'SELL','REDEMPTION','MERGER_OUT','SWITCH_OUT','TRANSFER_OUT','SPLIT_OUT'
         )
         AND (t.flag IS NULL OR t.flag_dismissed = 1)
         {extra}
         ORDER BY t.account_id, COALESCE(t.instrument_id, -t.pending_instrument_id), t.trade_date ASC, t.txn_id ASC"
    );

    let mut stmt = conn.prepare(&sql).map_err(|e| e.to_string())?;
    let rows: Vec<TxnRow> = stmt.query_map(
        rusqlite::params_from_iter(params.iter().map(|p| p.as_ref())),
        |row| Ok(TxnRow {
            txn_id:          row.get(0)?,
            account_id:      row.get(1)?,
            account_name:    row.get(2)?,
            portfolio_id:    row.get(3)?,
            instrument_id:   row.get(4)?,
            instrument_name: row.get(5)?,
            isin:            row.get(6)?,
            instrument_type: row.get(7)?,
            asset_class:     row.get(8)?,
            is_pending:      row.get::<_, i64>(9)? != 0,
            trade_date:      row.get(10)?,
            txn_type:        row.get(11)?,
            quantity:        row.get(12)?,
            price_paise:     row.get(13)?,
            current_price:   row.get(14)?,
            price_date:      row.get(15)?,
        }),
    )
    .map_err(|e| e.to_string())?
    .collect::<Result<Vec<_>, _>>()
    .map_err(|e| e.to_string())?;

    // Collapse all transactions per (account, instrument, date) into a single
    // net BUY or net SELL for that day, then run FIFO over those daily nets.
    let effective_rows = net_by_day(rows);

    // Group by (account_id, instrument_id) and apply FIFO matching
    let mut positions: HashMap<(i64, i64), (PositionMeta, Vec<BuyLot>)> = HashMap::new();

    for row in &effective_rows {
        let key = (row.account_id, row.instrument_id);

        let entry = positions.entry(key).or_insert_with(|| {
            (PositionMeta {
                account_name:    row.account_name.clone(),
                portfolio_id:    row.portfolio_id,
                instrument_name: row.instrument_name.clone(),
                isin:            row.isin.clone(),
                instrument_type: row.instrument_type.clone(),
                asset_class:     row.asset_class.clone(),
                is_pending:      row.is_pending,
                current_price:   row.current_price,
                price_date:      row.price_date.clone(),
            }, Vec::new())
        });

        let lots = &mut entry.1;

        if is_buy(&row.txn_type) {
            lots.push(BuyLot {
                price_paise:   row.price_paise,
                remaining_qty: row.quantity,
            });
        } else if is_sell(&row.txn_type) {
            let mut qty_to_match = row.quantity;
            for lot in lots.iter_mut() {
                if qty_to_match <= 0.0001 { break; }
                if lot.remaining_qty <= 0.0001 { continue; }
                let matched = qty_to_match.min(lot.remaining_qty);
                lot.remaining_qty -= matched;
                qty_to_match -= matched;
            }
        }
    }

    // Build holdings from remaining lots
    let mut holdings: Vec<Holding> = positions
        .into_iter()
        .filter_map(|((account_id, instrument_id), (meta, lots))| {
            let remaining_qty: f64 = lots.iter().map(|l| l.remaining_qty).sum();
            if remaining_qty <= 0.0001 {
                return None;
            }

            let total_cost_paise: i64 = lots.iter()
                .filter(|l| l.remaining_qty > 0.0001)
                .map(|l| (l.price_paise as f64 * l.remaining_qty).round() as i64)
                .sum();

            let avg_cost_paise = (total_cost_paise as f64 / remaining_qty).round() as i64;

            let current_value_paise =
                meta.current_price.map(|p| (p as f64 * remaining_qty).round() as i64);
            let unrealized_pnl_paise =
                current_value_paise.map(|cv| cv - total_cost_paise);
            let unrealized_pnl_pct = unrealized_pnl_paise.and_then(|pnl| {
                if total_cost_paise > 0 {
                    Some((pnl as f64 / total_cost_paise as f64) * 100.0)
                } else {
                    None
                }
            });

            Some(Holding {
                account_id,
                account_name:         meta.account_name,
                portfolio_id:         meta.portfolio_id,
                instrument_id,
                instrument_name:      meta.instrument_name,
                isin:                 meta.isin,
                instrument_type:      meta.instrument_type,
                asset_class:          meta.asset_class,
                is_pending:           meta.is_pending,
                quantity:             remaining_qty,
                avg_cost_paise,
                total_cost_paise,
                current_price_paise:  meta.current_price,
                current_value_paise,
                unrealized_pnl_paise,
                unrealized_pnl_pct,
                price_date:           meta.price_date,
            })
        })
        .collect();

    holdings.sort_by(|a, b| a.instrument_name.cmp(&b.instrument_name));

    Ok(holdings)
}

/// Collapses all transactions for the same (account, instrument, date) into a single
/// net quantity, regardless of trade segment. A net-buy day emits one synthetic BUY
/// at the weighted-average buy price; a net-sell day emits one synthetic SELL; a
/// fully-offsetting day emits nothing. The result is then sorted for FIFO input.
fn net_by_day(rows: Vec<TxnRow>) -> Vec<TxnRow> {
    let mut groups: HashMap<(i64, i64, String), Vec<TxnRow>> = HashMap::new();
    for row in rows {
        let key = (row.account_id, row.instrument_id, row.trade_date.clone());
        groups.entry(key).or_default().push(row);
    }

    let mut result: Vec<TxnRow> = Vec::new();

    for ((account_id, instrument_id, trade_date), group) in groups {
        let meta       = &group[0];
        let mut min_id = i64::MAX;
        let mut buy_qty   = 0.0f64;
        let mut buy_value = 0.0f64; // sum(qty * price_paise)
        let mut sell_qty  = 0.0f64;

        for row in &group {
            if row.txn_id < min_id { min_id = row.txn_id; }
            if is_buy(&row.txn_type) {
                buy_qty   += row.quantity;
                buy_value += row.quantity * row.price_paise as f64;
            } else if is_sell(&row.txn_type) {
                sell_qty += row.quantity;
            }
        }

        let net = buy_qty - sell_qty;

        if net > 0.0001 {
            let avg_price = (buy_value / buy_qty).round() as i64;
            result.push(TxnRow {
                txn_id:          min_id,
                account_id,
                account_name:    meta.account_name.clone(),
                portfolio_id:    meta.portfolio_id,
                instrument_id,
                instrument_name: meta.instrument_name.clone(),
                isin:            meta.isin.clone(),
                instrument_type: meta.instrument_type.clone(),
                asset_class:     meta.asset_class.clone(),
                is_pending:      meta.is_pending,
                trade_date:      trade_date.clone(),

                txn_type:        "BUY".to_string(),
                quantity:        net,
                price_paise:     avg_price,
                current_price:   meta.current_price,
                price_date:      meta.price_date.clone(),
            });
        } else if net < -0.0001 {
            result.push(TxnRow {
                txn_id:          min_id,
                account_id,
                account_name:    meta.account_name.clone(),
                portfolio_id:    meta.portfolio_id,
                instrument_id,
                instrument_name: meta.instrument_name.clone(),
                isin:            meta.isin.clone(),
                instrument_type: meta.instrument_type.clone(),
                asset_class:     meta.asset_class.clone(),
                is_pending:      meta.is_pending,
                trade_date:      trade_date.clone(),

                txn_type:        "SELL".to_string(),
                quantity:        -net,
                price_paise:     0,
                current_price:   meta.current_price,
                price_date:      meta.price_date.clone(),
            });
        }
        // net ≈ 0: fully offset, emit nothing
    }

    result.sort_by(|a, b| {
        a.account_id.cmp(&b.account_id)
            .then(a.instrument_id.cmp(&b.instrument_id))
            .then(a.trade_date.cmp(&b.trade_date))
            .then(a.txn_id.cmp(&b.txn_id))
    });
    result
}

/// Portfolio-level summary aggregated from holdings.
#[tauri::command]
pub fn get_portfolio_summary(
    account_ids:   Option<Vec<i64>>,
    portfolio_ids: Option<Vec<i64>>,
    asset_classes: Option<Vec<String>>,
) -> Result<PortfolioSummary, String> {
    let holdings = get_holdings(account_ids, portfolio_ids, asset_classes)?;

    let total_invested = holdings.iter().map(|h| h.total_cost_paise).sum::<i64>();
    let holdings_count = holdings.len() as i64;
    let accounts_count = holdings.iter()
        .map(|h| h.account_id)
        .collect::<std::collections::HashSet<_>>()
        .len() as i64;

    let current_value = if holdings.iter().any(|h| h.current_value_paise.is_some()) {
        Some(holdings.iter().filter_map(|h| h.current_value_paise).sum::<i64>())
    } else {
        None
    };

    let unrealized_pnl = current_value.map(|cv| cv - total_invested);
    let unrealized_pnl_pct = unrealized_pnl.and_then(|pnl| {
        if total_invested > 0 {
            Some((pnl as f64 / total_invested as f64) * 100.0)
        } else {
            None
        }
    });

    Ok(PortfolioSummary {
        total_invested_paise: total_invested,
        current_value_paise: current_value,
        unrealized_pnl_paise: unrealized_pnl,
        unrealized_pnl_pct,
        holdings_count,
        accounts_count,
    })
}
