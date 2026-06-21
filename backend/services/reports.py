"""Capital gains + income reports — port of src-tauri/src/commands/reports.rs."""
import sqlite3
from dataclasses import dataclass
from datetime import date, datetime


# ─── Helpers ─────────────────────────────────────────────────────────────────

def fy_of(iso_date: str) -> str:
    """Indian financial year (April–March) for an ISO date string."""
    try:
        d = date.fromisoformat(iso_date)
    except ValueError:
        return "2024-25"
    if d.month >= 4:
        return f"{d.year}-{(d.year + 1) % 100:02d}"
    return f"{d.year - 1}-{d.year % 100:02d}"


def today_iso() -> str:
    return date.today().isoformat()


def holding_days(buy: str, sell: str) -> int:
    try:
        return (date.fromisoformat(sell) - date.fromisoformat(buy)).days
    except ValueError:
        return 0


def gain_type_for(tax_category: str, trade_segment: str, days: int) -> str:
    if trade_segment == "INTRADAY":
        return "SPECULATIVE"
    if tax_category == "EQUITY_LTCG":
        return "LTCG" if days >= 365 else "STCG"
    if tax_category == "DEBT":
        return "LTCG" if days >= 1095 else "STCG"
    if tax_category in ("NON_SPECULATIVE", "SPECULATIVE"):
        return "NON_SPECULATIVE"
    return "LTCG" if days >= 365 else "STCG"


def same_day_gain_type(asset_class: str, tax_category: str) -> str:
    if asset_class == "EQUITY":
        return "SPECULATIVE"
    if tax_category == "NON_SPECULATIVE":
        return "NON_SPECULATIVE"
    return "STCG"


# ─── Capital Gains ────────────────────────────────────────────────────────────

_TXN_TYPES_CG = (
    "BUY", "SIP", "IPO", "FPO", "OPENING_BALANCE", "BONUS",
    "MERGER_IN", "SWITCH_IN", "TRANSFER_IN", "SPLIT_IN",
    "SELL", "REDEMPTION", "MERGER_OUT", "SWITCH_OUT", "TRANSFER_OUT", "SPLIT_OUT",
)

_BUY_TYPES = {
    "BUY", "SIP", "IPO", "FPO", "OPENING_BALANCE", "BONUS",
    "MERGER_IN", "SWITCH_IN", "TRANSFER_IN", "SPLIT_IN",
}
_TAXABLE_SELL = {"SELL", "REDEMPTION", "MERGER_OUT", "SWITCH_OUT"}
_SILENT_SELL  = {"TRANSFER_OUT", "SPLIT_OUT"}
_TRADING_BUY  = {"BUY", "IPO", "FPO", "SIP"}
_TRADING_SELL = {"SELL", "REDEMPTION"}


def get_capital_gains(
    conn: sqlite3.Connection,
    fy: str | None = None,
    account_ids: list[int] | None = None,
) -> dict:
    acct_clause = ""
    params: list = []
    if account_ids:
        placeholders = ",".join("?" * len(account_ids))
        acct_clause = f"AND t.account_id IN ({placeholders})"
        params.extend(account_ids)

    placeholders_str = ",".join(f"'{t}'" for t in _TXN_TYPES_CG)

    sql = f"""
        SELECT t.account_id,
               a.name AS account_name,
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
               t.quantity, t.effective_price_paise
        FROM transactions t
        JOIN accounts a ON t.account_id = a.account_id
        LEFT JOIN instruments i ON i.instrument_id = t.instrument_id
        LEFT JOIN instrument_types it ON it.instrument_type_id = i.instrument_type_id
        LEFT JOIN instrument_equity ie ON ie.instrument_id = i.instrument_id
        LEFT JOIN pending_instruments pi ON pi.pending_id = t.pending_instrument_id
        WHERE t.txn_type IN ({placeholders_str})
        {acct_clause}
        ORDER BY t.account_id,
                 COALESCE(t.instrument_id, -t.pending_instrument_id),
                 t.trade_date ASC, t.txn_id ASC
    """

    rows = conn.execute(sql, params).fetchall()

    # Phase 1: same-day netting — compute buy/sell day totals per (account, instrument, date)
    day_map: dict[tuple, dict] = {}
    for row in rows:
        if row["txn_type"] not in _TRADING_BUY and row["txn_type"] not in _TRADING_SELL:
            continue
        key = (row["account_id"], row["instrument_id"], row["trade_date"])
        if key not in day_map:
            day_map[key] = {
                "buy_qty": 0.0, "buy_value": 0.0,
                "sell_qty": 0.0, "sell_value": 0.0,
                "instrument_id": row["instrument_id"],
                "instrument_name": row["instrument_name"] or "",
                "isin": row["isin"],
                "asset_class": row["asset_class"] or "EQUITY",
                "tax_category": row["tax_category"] or "EQUITY_LTCG",
                "account_name": row["account_name"],
            }
        d = day_map[key]
        if row["txn_type"] in _TRADING_BUY:
            d["buy_qty"] += row["quantity"]
            d["buy_value"] += row["quantity"] * row["effective_price_paise"]
        else:
            d["sell_qty"] += row["quantity"]
            d["sell_value"] += row["quantity"] * row["effective_price_paise"]

    lots: list[dict] = []

    # Phase 2: emit same-day matched lots
    for key, d in sorted(day_map.items()):
        matched_qty = min(d["buy_qty"], d["sell_qty"])
        if matched_qty < 1e-4:
            continue
        avg_buy  = d["buy_value"]  / d["buy_qty"]  if d["buy_qty"]  > 0 else 0.0
        avg_sell = d["sell_value"] / d["sell_qty"] if d["sell_qty"] > 0 else 0.0
        cost     = round(avg_buy  * matched_qty)
        proceeds = round(avg_sell * matched_qty)
        gt = same_day_gain_type(d["asset_class"], d["tax_category"])
        trade_date = key[2]
        lots.append({
            "instrument_id": d["instrument_id"],
            "instrument_name": d["instrument_name"],
            "isin": d["isin"],
            "asset_class": d["asset_class"],
            "tax_category": d["tax_category"],
            "account_name": d["account_name"],
            "buy_date": trade_date, "sell_date": trade_date,
            "quantity": matched_qty,
            "buy_price_paise": int(avg_buy), "sell_price_paise": int(avg_sell),
            "cost_paise": cost, "proceeds_paise": proceeds,
            "gain_paise": proceeds - cost,
            "holding_days": 0, "gain_type": gt, "fy": fy_of(trade_date),
        })

    # Phase 3: delivery FIFO
    # Track how much of each day's trading qty was consumed by same-day matching
    intra_budget: dict[tuple, dict] = {}
    for key, d in day_map.items():
        matched_qty = min(d["buy_qty"], d["sell_qty"])
        if matched_qty > 1e-4:
            intra_budget[key] = {"buy_rem": matched_qty, "sell_rem": matched_qty}

    @dataclass
    class BuyLot:
        trade_date: str
        cost_per_unit_paise: float
        remaining_qty: float

    buy_queues: dict[tuple, list[BuyLot]] = {}

    for row in rows:
        acct_id  = row["account_id"]
        instr_id = row["instrument_id"]
        key      = (acct_id, instr_id)
        bkey     = (acct_id, instr_id, row["trade_date"])
        qty      = row["quantity"]
        eff      = row["effective_price_paise"]

        if row["txn_type"] in _BUY_TYPES:
            delivery_qty = qty
            if row["txn_type"] in _TRADING_BUY and bkey in intra_budget:
                used = min(qty, intra_budget[bkey]["buy_rem"])
                intra_budget[bkey]["buy_rem"] -= used
                delivery_qty = qty - used
            if delivery_qty < 1e-4:
                continue
            buy_queues.setdefault(key, []).append(
                BuyLot(row["trade_date"], eff, delivery_qty)
            )

        elif row["txn_type"] in _TAXABLE_SELL or row["txn_type"] in _SILENT_SELL:
            taxable = row["txn_type"] in _TAXABLE_SELL
            delivery_qty = qty
            if row["txn_type"] in _TRADING_SELL and bkey in intra_budget:
                used = min(qty, intra_budget[bkey]["sell_rem"])
                intra_budget[bkey]["sell_rem"] -= used
                delivery_qty = qty - used
            if delivery_qty < 1e-4:
                continue

            sell_per_unit = eff
            queue = buy_queues.setdefault(key, [])
            qty_to_match = delivery_qty

            for lot in queue:
                if qty_to_match <= 1e-4:
                    break
                if lot.remaining_qty <= 1e-4:
                    continue
                matched = min(qty_to_match, lot.remaining_qty)
                lot.remaining_qty -= matched
                qty_to_match -= matched

                if not taxable:
                    continue

                cost     = round(lot.cost_per_unit_paise * matched)
                proceeds = round(sell_per_unit * matched)
                days = holding_days(lot.trade_date, row["trade_date"])
                gt   = gain_type_for(
                    row["tax_category"] or "EQUITY_LTCG", "DELIVERY", days
                )
                lots.append({
                    "instrument_id": row["instrument_id"],
                    "instrument_name": row["instrument_name"] or "",
                    "isin": row["isin"],
                    "asset_class": row["asset_class"] or "EQUITY",
                    "tax_category": row["tax_category"] or "EQUITY_LTCG",
                    "account_name": row["account_name"],
                    "buy_date": lot.trade_date, "sell_date": row["trade_date"],
                    "quantity": matched,
                    "buy_price_paise": int(lot.cost_per_unit_paise),
                    "sell_price_paise": int(sell_per_unit),
                    "cost_paise": cost, "proceeds_paise": proceeds,
                    "gain_paise": proceeds - cost,
                    "holding_days": days, "gain_type": gt,
                    "fy": fy_of(row["trade_date"]),
                })

    lots.sort(key=lambda l: l["sell_date"], reverse=True)

    all_fys = sorted(set(l["fy"] for l in lots), reverse=True)

    # Per-FY summaries
    def summarise(f: str) -> dict:
        fy_lots = [l for l in lots if l["fy"] == f]
        stcg_eq = ltcg_eq = stcg_debt = ltcg_debt = spec = non_spec = 0
        for l in fy_lots:
            g = l["gain_paise"]
            match l["gain_type"]:
                case "STCG":
                    if l["tax_category"] == "DEBT":
                        stcg_debt += g
                    else:
                        stcg_eq += g
                case "LTCG":
                    if l["tax_category"] == "DEBT":
                        ltcg_debt += g
                    else:
                        ltcg_eq += g
                case "SPECULATIVE":
                    spec += g
                case "NON_SPECULATIVE":
                    non_spec += g
                case _:
                    stcg_eq += g
        total = stcg_eq + ltcg_eq + stcg_debt + ltcg_debt + spec + non_spec
        return {
            "fy": f,
            "stcg_equity_paise": stcg_eq, "ltcg_equity_paise": ltcg_eq,
            "stcg_debt_paise": stcg_debt, "ltcg_debt_paise": ltcg_debt,
            "speculative_paise": spec, "non_speculative_paise": non_spec,
            "total_gain_paise": total,
        }

    fys_to_summarise = all_fys if all_fys else [fy_of(today_iso())]
    summaries = [summarise(f) for f in fys_to_summarise]

    filtered_lots = [l for l in lots if l["fy"] == fy] if fy else lots
    return {"lots": filtered_lots, "summaries": summaries, "all_fys": all_fys}


# ─── Income ───────────────────────────────────────────────────────────────────

def get_income(
    conn: sqlite3.Connection,
    fy: str | None = None,
    account_ids: list[int] | None = None,
) -> dict:
    acct_clause = ""
    params: list = []
    if account_ids:
        placeholders = ",".join("?" * len(account_ids))
        acct_clause = f"AND t.account_id IN ({placeholders})"
        params.extend(account_ids)

    sql = f"""
        SELECT t.txn_id,
               COALESCE(t.instrument_id, -t.pending_instrument_id) AS instrument_id,
               COALESCE(i.name, pi.name) AS instrument_name,
               ie.isin,
               COALESCE(it.asset_class, 'EQUITY') AS asset_class,
               a.name AS account_name,
               t.txn_type, t.trade_date,
               CAST(ROUND(t.quantity * t.effective_price_paise) AS INTEGER) AS total_value_paise,
               t.notes
        FROM transactions t
        JOIN accounts a ON t.account_id = a.account_id
        LEFT JOIN instruments i ON t.instrument_id = i.instrument_id
        LEFT JOIN instrument_types it ON i.instrument_type_id = it.instrument_type_id
        LEFT JOIN instrument_equity ie ON ie.instrument_id = i.instrument_id
        LEFT JOIN pending_instruments pi ON pi.pending_id = t.pending_instrument_id
        WHERE t.txn_type IN ('DIVIDEND', 'INTEREST')
        {acct_clause}
        ORDER BY t.trade_date DESC, t.txn_id DESC
    """

    rows = conn.execute(sql, params).fetchall()
    events = [
        {
            "txn_id": r["txn_id"],
            "instrument_id": r["instrument_id"],
            "instrument_name": r["instrument_name"] or "",
            "isin": r["isin"],
            "asset_class": r["asset_class"],
            "account_name": r["account_name"],
            "income_type": r["txn_type"],
            "trade_date": r["trade_date"],
            "amount_paise": abs(r["total_value_paise"]),
            "notes": r["notes"],
            "fy": fy_of(r["trade_date"]),
        }
        for r in rows
    ]

    all_fys = sorted(set(e["fy"] for e in events), reverse=True)

    summaries = [
        {
            "fy": f,
            "dividend_paise": sum(e["amount_paise"] for e in events if e["fy"] == f and e["income_type"] == "DIVIDEND"),
            "interest_paise": sum(e["amount_paise"] for e in events if e["fy"] == f and e["income_type"] == "INTEREST"),
            "total_paise": sum(e["amount_paise"] for e in events if e["fy"] == f),
        }
        for f in all_fys
    ]

    filtered = [e for e in events if e["fy"] == fy] if fy else events
    return {"events": filtered, "summaries": summaries, "all_fys": all_fys}


# ─── Excel Export ─────────────────────────────────────────────────────────────

def export_tax_report(
    conn: sqlite3.Connection, fy: str, dest_path: str
) -> None:
    import xlsxwriter

    cg     = get_capital_gains(conn, fy=fy)
    income = get_income(conn, fy=fy)
    summary = next((s for s in cg["summaries"] if s["fy"] == fy), {
        "fy": fy,
        "stcg_equity_paise": 0, "ltcg_equity_paise": 0,
        "stcg_debt_paise": 0, "ltcg_debt_paise": 0,
        "speculative_paise": 0, "non_speculative_paise": 0,
        "total_gain_paise": 0,
    })

    def paise_to_rupees(p: int) -> float:
        return p / 100.0

    wb = xlsxwriter.Workbook(dest_path)
    hdr_fmt   = wb.add_format({"bold": True, "bg_color": "#1e293b", "font_color": "#f8fafc",
                                "border": 1, "align": "center"})
    label_fmt = wb.add_format({"bold": True, "bg_color": "#f1f5f9", "border": 1})
    money_fmt = wb.add_format({"num_format": "₹#,##0.00", "border": 1})
    pos_fmt   = wb.add_format({"num_format": "₹#,##0.00", "font_color": "#16a34a",
                                "bold": True, "border": 1})
    neg_fmt   = wb.add_format({"num_format": "₹#,##0.00", "font_color": "#dc2626",
                                "bold": True, "border": 1})
    cell_fmt  = wb.add_format({"border": 1})
    num_fmt   = wb.add_format({"num_format": "#,##0.####", "border": 1})

    # Sheet 1: CG Summary
    ws1 = wb.add_worksheet("CG Summary")
    ws1.set_column(0, 0, 30)
    ws1.set_column(1, 1, 18)
    ws1.write(0, 0, f"Capital Gains Summary – FY {fy}",
              wb.add_format({"bold": True, "font_size": 13}))

    rows_data = [
        ("STCG – Equity",         summary["stcg_equity_paise"]),
        ("LTCG – Equity",         summary["ltcg_equity_paise"]),
        ("STCG – Debt",           summary["stcg_debt_paise"]),
        ("LTCG – Debt",           summary["ltcg_debt_paise"]),
        ("Speculative Gains",     summary["speculative_paise"]),
        ("Non-Speculative Gains", summary["non_speculative_paise"]),
        ("Total Realized Gain",   summary["total_gain_paise"]),
    ]
    for i, (label, paise) in enumerate(rows_data):
        r = i + 2
        rupees = paise_to_rupees(paise)
        fmt = (pos_fmt if rupees >= 0 else neg_fmt) if label == "Total Realized Gain" else money_fmt
        ws1.write(r, 0, label, label_fmt)
        ws1.write(r, 1, rupees, fmt)

    # Sheet 2: CG Details
    ws2 = wb.add_worksheet("CG Details")
    headers = ["Instrument", "ISIN", "Account", "Type", "Asset Class",
               "Buy Date", "Sell Date", "Hold Days", "Qty",
               "Buy Price (₹)", "Sell Price (₹)", "Cost (₹)", "Proceeds (₹)", "Gain (₹)", "Gain Type"]
    widths  = [28, 14, 18, 14, 14, 13, 13, 10, 10, 14, 14, 14, 14, 14, 16]
    for i, (h, w) in enumerate(zip(headers, widths)):
        ws2.set_column(i, i, w)
        ws2.write(0, i, h, hdr_fmt)

    for i, lot in enumerate(cg["lots"]):
        r = i + 1
        gain_fmt = pos_fmt if lot["gain_paise"] >= 0 else neg_fmt
        ws2.write(r, 0,  lot["instrument_name"], cell_fmt)
        ws2.write(r, 1,  lot["isin"] or "",      cell_fmt)
        ws2.write(r, 2,  lot["account_name"],    cell_fmt)
        ws2.write(r, 3,  lot["tax_category"],    cell_fmt)
        ws2.write(r, 4,  lot["asset_class"],     cell_fmt)
        ws2.write(r, 5,  lot["buy_date"],        cell_fmt)
        ws2.write(r, 6,  lot["sell_date"],       cell_fmt)
        ws2.write(r, 7,  lot["holding_days"],    cell_fmt)
        ws2.write(r, 8,  lot["quantity"],        num_fmt)
        ws2.write(r, 9,  paise_to_rupees(lot["buy_price_paise"]),  money_fmt)
        ws2.write(r, 10, paise_to_rupees(lot["sell_price_paise"]), money_fmt)
        ws2.write(r, 11, paise_to_rupees(lot["cost_paise"]),       money_fmt)
        ws2.write(r, 12, paise_to_rupees(lot["proceeds_paise"]),   money_fmt)
        ws2.write(r, 13, paise_to_rupees(lot["gain_paise"]),       gain_fmt)
        ws2.write(r, 14, lot["gain_type"],       cell_fmt)

    # Sheet 3: Income
    ws3 = wb.add_worksheet("Income")
    inc_headers = ["Date", "Instrument", "ISIN", "Account", "Type", "Amount (₹)", "Notes"]
    inc_widths  = [13, 28, 14, 18, 12, 14, 30]
    for i, (h, w) in enumerate(zip(inc_headers, inc_widths)):
        ws3.set_column(i, i, w)
        ws3.write(0, i, h, hdr_fmt)

    for i, ev in enumerate(income["events"]):
        r = i + 1
        ws3.write(r, 0, ev["trade_date"],            cell_fmt)
        ws3.write(r, 1, ev["instrument_name"],       cell_fmt)
        ws3.write(r, 2, ev["isin"] or "",            cell_fmt)
        ws3.write(r, 3, ev["account_name"],          cell_fmt)
        ws3.write(r, 4, ev["income_type"],           cell_fmt)
        ws3.write(r, 5, paise_to_rupees(ev["amount_paise"]), pos_fmt)
        ws3.write(r, 6, ev["notes"] or "",           cell_fmt)

    wb.close()
