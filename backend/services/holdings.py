"""FIFO holdings engine — port of src-tauri/src/commands/holdings.rs."""
import sqlite3
from collections import defaultdict
from dataclasses import dataclass, field

_BUY_TYPES = {
    "BUY", "SIP", "IPO", "FPO", "OPENING_BALANCE", "BONUS",
    "MERGER_IN", "SWITCH_IN", "TRANSFER_IN", "SPLIT_IN",
}
_SELL_TYPES = {
    "SELL", "REDEMPTION", "MERGER_OUT", "SWITCH_OUT", "TRANSFER_OUT", "SPLIT_OUT",
}

_PENDING_ASSET_CLASS = {
    "EQUITY": "EQUITY", "MF": "MF",
    "FUTSTK": "DERIVATIVE", "FUTIDX": "DERIVATIVE",
    "OPTSTK": "DERIVATIVE", "OPTIDX": "DERIVATIVE",
    "MCX": "COMMODITY",
}


@dataclass
class TxnRow:
    txn_id: int
    account_id: int
    account_name: str
    portfolio_id: int
    instrument_id: int  # negative for pending
    instrument_name: str
    isin: str | None
    instrument_type: str
    asset_class: str
    is_pending: bool
    trade_date: str
    txn_type: str
    quantity: float
    effective_price_paise: int
    current_price: int | None
    price_date: str | None


@dataclass
class BuyLot:
    effective_price_paise: int
    remaining_qty: float


def _net_by_day(rows: list[TxnRow]) -> list[TxnRow]:
    """
    Collapse all transactions for the same (account, instrument, date) into a single
    net daily quantity. This handles intraday netting:
      - net > 0 → synthetic BUY at weighted-average buy price
      - net < 0 → synthetic SELL
      - net ≈ 0 → fully offset, emit nothing
    """
    groups: dict[tuple, list[TxnRow]] = defaultdict(list)
    for row in rows:
        key = (row.account_id, row.instrument_id, row.trade_date)
        groups[key].append(row)

    result: list[TxnRow] = []

    for (account_id, instrument_id, trade_date), group in groups.items():
        meta = group[0]
        min_id = min(r.txn_id for r in group)
        buy_qty = buy_value = sell_qty = 0.0

        for row in group:
            if row.txn_type in _BUY_TYPES:
                buy_qty += row.quantity
                buy_value += row.quantity * row.effective_price_paise
            elif row.txn_type in _SELL_TYPES:
                sell_qty += row.quantity

        net = buy_qty - sell_qty

        if net > 1e-4:
            avg_price = round(buy_value / buy_qty) if buy_qty > 0 else 0
            result.append(TxnRow(
                txn_id=min_id, account_id=account_id,
                account_name=meta.account_name, portfolio_id=meta.portfolio_id,
                instrument_id=instrument_id, instrument_name=meta.instrument_name,
                isin=meta.isin, instrument_type=meta.instrument_type,
                asset_class=meta.asset_class, is_pending=meta.is_pending,
                trade_date=trade_date, txn_type="BUY",
                quantity=net, effective_price_paise=avg_price,
                current_price=meta.current_price, price_date=meta.price_date,
            ))
        elif net < -1e-4:
            result.append(TxnRow(
                txn_id=min_id, account_id=account_id,
                account_name=meta.account_name, portfolio_id=meta.portfolio_id,
                instrument_id=instrument_id, instrument_name=meta.instrument_name,
                isin=meta.isin, instrument_type=meta.instrument_type,
                asset_class=meta.asset_class, is_pending=meta.is_pending,
                trade_date=trade_date, txn_type="SELL",
                quantity=-net, effective_price_paise=0,
                current_price=meta.current_price, price_date=meta.price_date,
            ))
        # net ≈ 0: fully offset intraday, emit nothing

    result.sort(key=lambda r: (r.account_id, r.instrument_id, r.trade_date, r.txn_id))
    return result


def compute_holdings(
    conn: sqlite3.Connection,
    account_ids: list[int] | None = None,
    portfolio_ids: list[int] | None = None,
    asset_classes: list[str] | None = None,
) -> list[dict]:
    """Compute current open positions using FIFO with intraday netting."""
    clauses = []
    params: list = []

    if account_ids:
        placeholders = ",".join("?" * len(account_ids))
        clauses.append(f"t.account_id IN ({placeholders})")
        params.extend(account_ids)

    if portfolio_ids:
        placeholders = ",".join("?" * len(portfolio_ids))
        clauses.append(f"a.portfolio_id IN ({placeholders})")
        params.extend(portfolio_ids)

    if asset_classes:
        placeholders = ",".join("?" * len(asset_classes))
        clauses.append(
            f"""COALESCE(it.asset_class, CASE pi.type
                WHEN 'EQUITY' THEN 'EQUITY'
                WHEN 'MF'     THEN 'MF'
                WHEN 'FUTSTK' THEN 'DERIVATIVE'
                WHEN 'FUTIDX' THEN 'DERIVATIVE'
                WHEN 'OPTSTK' THEN 'DERIVATIVE'
                WHEN 'OPTIDX' THEN 'DERIVATIVE'
                WHEN 'MCX'    THEN 'COMMODITY'
                ELSE 'UNKNOWN' END) IN ({placeholders})"""
        )
        params.extend(asset_classes)

    extra = ("AND " + " AND ".join(clauses)) if clauses else ""

    sql = f"""
        SELECT t.txn_id, t.account_id, a.name AS account_name, a.portfolio_id,
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
               t.txn_type, t.quantity, t.effective_price_paise,
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
        ORDER BY t.account_id,
                 COALESCE(t.instrument_id, -t.pending_instrument_id),
                 t.trade_date ASC, t.txn_id ASC
    """

    raw_rows = conn.execute(sql, params).fetchall()

    txn_rows = [
        TxnRow(
            txn_id=r["txn_id"],
            account_id=r["account_id"],
            account_name=r["account_name"],
            portfolio_id=r["portfolio_id"],
            instrument_id=r["instrument_id"],
            instrument_name=r["instrument_name"] or "",
            isin=r["isin"],
            instrument_type=r["instrument_type"] or "",
            asset_class=r["asset_class"] or "UNKNOWN",
            is_pending=bool(r["is_pending"]),
            trade_date=r["trade_date"],
            txn_type=r["txn_type"],
            quantity=r["quantity"],
            effective_price_paise=r["effective_price_paise"],
            current_price=r["close_price_paise"],
            price_date=r["price_date"],
        )
        for r in raw_rows
    ]

    effective = _net_by_day(txn_rows)

    # FIFO matching: group by (account_id, instrument_id), maintain buy-lot queue
    positions: dict[tuple, tuple[TxnRow, list[BuyLot]]] = {}

    for row in effective:
        key = (row.account_id, row.instrument_id)
        if key not in positions:
            positions[key] = (row, [])

        meta, lots = positions[key]

        if row.txn_type in _BUY_TYPES or row.txn_type == "BUY":
            lots.append(BuyLot(effective_price_paise=row.effective_price_paise, remaining_qty=row.quantity))
        elif row.txn_type in _SELL_TYPES or row.txn_type == "SELL":
            qty_to_match = row.quantity
            for lot in lots:
                if qty_to_match <= 1e-4:
                    break
                if lot.remaining_qty <= 1e-4:
                    continue
                matched = min(qty_to_match, lot.remaining_qty)
                lot.remaining_qty -= matched
                qty_to_match -= matched

    # Build output
    holdings = []
    for (account_id, instrument_id), (meta, lots) in positions.items():
        remaining_qty = sum(l.remaining_qty for l in lots)
        if remaining_qty <= 1e-4:
            continue

        total_cost_paise = sum(
            round(l.effective_price_paise * l.remaining_qty)
            for l in lots if l.remaining_qty > 1e-4
        )
        avg_cost_paise = round(total_cost_paise / remaining_qty) if remaining_qty > 0 else 0

        current_value_paise = (
            round(meta.current_price * remaining_qty) if meta.current_price else None
        )
        unrealized_pnl_paise = (
            current_value_paise - total_cost_paise if current_value_paise is not None else None
        )
        unrealized_pnl_pct = (
            (unrealized_pnl_paise / total_cost_paise * 100.0)
            if unrealized_pnl_paise is not None and total_cost_paise > 0
            else None
        )

        holdings.append({
            "instrument_id": instrument_id,
            "instrument_name": meta.instrument_name,
            "isin": meta.isin,
            "instrument_type": meta.instrument_type,
            "asset_class": meta.asset_class,
            "is_pending": meta.is_pending,
            "account_id": account_id,
            "account_name": meta.account_name,
            "portfolio_id": meta.portfolio_id,
            "quantity": remaining_qty,
            "avg_cost_paise": avg_cost_paise,
            "total_cost_paise": total_cost_paise,
            "current_price_paise": meta.current_price,
            "current_value_paise": current_value_paise,
            "unrealized_pnl_paise": unrealized_pnl_paise,
            "unrealized_pnl_pct": unrealized_pnl_pct,
            "price_date": meta.price_date,
        })

    holdings.sort(key=lambda h: h["instrument_name"])
    return holdings


def compute_portfolio_summary(
    conn: sqlite3.Connection,
    account_ids: list[int] | None = None,
    portfolio_ids: list[int] | None = None,
    asset_classes: list[str] | None = None,
) -> dict:
    holdings = compute_holdings(conn, account_ids, portfolio_ids, asset_classes)

    total_invested = sum(h["total_cost_paise"] for h in holdings)
    holdings_count = len(holdings)
    accounts_count = len({h["account_id"] for h in holdings})

    has_prices = any(h["current_value_paise"] is not None for h in holdings)
    current_value = (
        sum(h["current_value_paise"] or 0 for h in holdings) if has_prices else None
    )
    unrealized_pnl = (current_value - total_invested) if current_value is not None else None
    unrealized_pnl_pct = (
        (unrealized_pnl / total_invested * 100.0)
        if unrealized_pnl is not None and total_invested > 0
        else None
    )

    return {
        "total_invested_paise": total_invested,
        "current_value_paise": current_value,
        "unrealized_pnl_paise": unrealized_pnl,
        "unrealized_pnl_pct": unrealized_pnl_pct,
        "holdings_count": holdings_count,
        "accounts_count": accounts_count,
    }
