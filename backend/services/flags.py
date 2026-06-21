"""Oversell detection — port of src-tauri/src/commands/import/mod.rs::flag_oversells."""
import sqlite3
from collections import defaultdict

_BUY_TYPES = {
    "BUY", "SIP", "IPO", "FPO", "OPENING_BALANCE", "BONUS",
    "MERGER_IN", "SWITCH_IN", "TRANSFER_IN", "SPLIT_IN",
}
_SELL_TYPES = {
    "SELL", "REDEMPTION", "MERGER_OUT", "SWITCH_OUT", "TRANSFER_OUT", "SPLIT_OUT",
}
_ALL_RELEVANT = _BUY_TYPES | _SELL_TYPES


def flag_oversells(conn: sqlite3.Connection, account_id: int) -> None:
    """
    Group transactions by (instrument, date) to compute daily net quantities,
    then flag any day where sells exceed available holding.
    TRANSFER_OUT oversells also flag the paired TRANSFER_IN via broker_ref.
    """
    rows = conn.execute(
        """SELECT t.txn_id,
                  COALESCE(t.instrument_id, -t.pending_instrument_id) AS instrument_id,
                  t.trade_date, t.txn_type, t.quantity
           FROM transactions t
           WHERE t.account_id = ?
             AND t.txn_type IN (
                 'BUY','SIP','IPO','FPO','OPENING_BALANCE','BONUS',
                 'MERGER_IN','SWITCH_IN','TRANSFER_IN','SPLIT_IN',
                 'SELL','REDEMPTION','MERGER_OUT','SWITCH_OUT','TRANSFER_OUT','SPLIT_OUT'
             )
             AND (t.flag IS NULL OR t.flag = 'OVERSELL')
           ORDER BY COALESCE(t.instrument_id, -t.pending_instrument_id),
                    t.trade_date ASC, t.txn_id ASC""",
        (account_id,),
    ).fetchall()

    # Group by (instrument_id, trade_date)
    day_map: dict[tuple, dict] = {}
    for row in rows:
        key = (row["instrument_id"], row["trade_date"])
        if key not in day_map:
            day_map[key] = {"buy_qty": 0.0, "sell_qty": 0.0, "all_ids": [], "sell_ids": []}
        g = day_map[key]
        g["all_ids"].append(row["txn_id"])
        if row["txn_type"] in _SELL_TYPES:
            g["sell_qty"] += row["quantity"]
            g["sell_ids"].append(row["txn_id"])
        else:
            g["buy_qty"] += row["quantity"]

    # Process in (instrument_id, trade_date) order — same as holdings FIFO
    qty_map: dict[int, float] = defaultdict(float)
    to_flag:  list[tuple[int, str]] = []
    to_clear: list[int] = []

    for (instrument_id, _trade_date), g in sorted(day_map.items()):
        avail = qty_map[instrument_id]
        total_avail = avail + g["buy_qty"]

        if g["sell_qty"] > total_avail + 1e-4:
            reason = f"Day sells {g['sell_qty']:.4f} exceed available {total_avail:.4f}"
            for txn_id in g["sell_ids"]:
                to_flag.append((txn_id, reason))
            sell_set = set(g["sell_ids"])
            for txn_id in g["all_ids"]:
                if txn_id not in sell_set:
                    to_clear.append(txn_id)
            qty_map[instrument_id] += g["buy_qty"]
        else:
            qty_map[instrument_id] = total_avail - g["sell_qty"]
            to_clear.extend(g["all_ids"])

    # Apply in a single transaction
    for txn_id, reason in to_flag:
        conn.execute(
            """UPDATE transactions
               SET flag='OVERSELL', flag_reason=?, flag_dismissed=0
               WHERE txn_id=? AND (flag IS NULL OR flag='OVERSELL')""",
            (reason, txn_id),
        )
        # Flag paired TRANSFER_IN when TRANSFER_OUT is oversold
        conn.execute(
            """UPDATE transactions
               SET flag='PAIRED_OVERSELL',
                   flag_reason='Paired TRANSFER_OUT is oversold — transfer is invalid',
                   flag_dismissed=0
               WHERE broker_ref = (SELECT broker_ref FROM transactions WHERE txn_id=?)
                 AND txn_type='TRANSFER_IN'
                 AND broker_ref IS NOT NULL
                 AND (flag IS NULL OR flag='PAIRED_OVERSELL')""",
            (txn_id,),
        )

    for txn_id in to_clear:
        conn.execute(
            "UPDATE transactions SET flag=NULL, flag_reason=NULL WHERE txn_id=? AND flag='OVERSELL'",
            (txn_id,),
        )
        conn.execute(
            """UPDATE transactions SET flag=NULL, flag_reason=NULL
               WHERE broker_ref=(SELECT broker_ref FROM transactions WHERE txn_id=?)
                 AND txn_type='TRANSFER_IN'
                 AND broker_ref IS NOT NULL
                 AND flag='PAIRED_OVERSELL'""",
            (txn_id,),
        )

    conn.commit()


def re_evaluate_all(conn: sqlite3.Connection, account_id: int | None = None) -> None:
    if account_id is not None:
        flag_oversells(conn, account_id)
    else:
        account_ids = [r[0] for r in conn.execute("SELECT account_id FROM accounts").fetchall()]
        for aid in account_ids:
            flag_oversells(conn, aid)
