import json
import sqlite3
import time
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from routers.deps import get_conn
from services.flags import flag_oversells, re_evaluate_all

router = APIRouter(tags=["transactions"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]

_TXN_SELECT = """
    SELECT t.txn_id, t.account_id, a.name AS account_name,
           a.portfolio_id,
           COALESCE(t.instrument_id, -t.pending_instrument_id) AS instrument_id,
           COALESCE(i.name, pi.name)                           AS instrument_name,
           ie.isin,
           t.txn_type, t.trade_segment, t.trade_date, t.txn_time,
           t.quantity,
           t.actual_price_paise, t.brokerage_per_unit_paise, t.effective_price_paise,
           t.stt_paise, t.other_charges_paise,
           t.notes, t.broker_ref,
           t.flag, t.flag_reason, t.flag_dismissed,
           t.batch_id
    FROM transactions t
    JOIN accounts a ON t.account_id = a.account_id
    LEFT JOIN instruments i ON i.instrument_id = t.instrument_id
    LEFT JOIN pending_instruments pi ON pi.pending_id = t.pending_instrument_id
    LEFT JOIN instrument_equity ie ON ie.instrument_id = t.instrument_id
"""

_SORT_COLS = {
    "instrument_name": "COALESCE(i.name, pi.name)",
    "quantity": "t.quantity",
    "effective_price_paise": "t.effective_price_paise",
    "trade_date": "t.trade_date",
}

_BUY_TYPES = {
    "BUY", "SIP", "IPO", "FPO", "OPENING_BALANCE",
    "TRANSFER_IN", "SPLIT_IN", "MERGER_IN", "SWITCH_IN",
}


_BUY_LIKE = {
    "BUY", "SIP", "IPO", "FPO", "OPENING_BALANCE",
    "TRANSFER_IN", "SPLIT_IN", "MERGER_IN", "SWITCH_IN",
}
_SELL_LIKE = {
    "SELL", "REDEMPTION", "TRANSFER_OUT", "SPLIT_OUT",
    "MERGER_OUT", "SWITCH_OUT",
}


def _build_filter_sql(filter: dict) -> tuple[str, list]:
    """Return (WHERE clause additions, params list)."""
    clauses = []
    params = []

    account_ids = filter.get("account_ids") or []
    if account_ids:
        placeholders = ",".join("?" * len(account_ids))
        clauses.append(f"t.account_id IN ({placeholders})")
        params.extend(account_ids)

    instrument_id = filter.get("instrument_id")
    if instrument_id is not None:
        if instrument_id < 0:
            clauses.append("t.pending_instrument_id = ?")
            params.append(-instrument_id)
        else:
            clauses.append("t.instrument_id = ?")
            params.append(instrument_id)

    from_date = filter.get("from_date")
    if from_date:
        clauses.append("t.trade_date >= ?")
        params.append(from_date)

    to_date = filter.get("to_date")
    if to_date:
        clauses.append("t.trade_date <= ?")
        params.append(to_date)

    txn_type = filter.get("txn_type")
    if txn_type:
        clauses.append("t.txn_type = ?")
        params.append(txn_type)

    flag_filter = filter.get("flag_filter")
    if flag_filter == "flagged":
        clauses.append("t.flag IS NOT NULL AND t.flag_dismissed = 0")
    elif flag_filter == "clean":
        clauses.append("(t.flag IS NULL OR t.flag_dismissed = 1)")

    search = (filter.get("search") or "").strip()
    if search:
        term = f"%{search}%"
        clauses.append("(COALESCE(i.name, pi.name) LIKE ? OR a.name LIKE ? OR t.txn_type LIKE ?)")
        params.extend([term, term, term])

    where = " AND ".join(clauses)
    return where, params


def _get_txn(conn: sqlite3.Connection, txn_id: int) -> dict:
    row = conn.execute(
        _TXN_SELECT + " WHERE t.txn_id = ?", (txn_id,)
    ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="Transaction not found")
    return dict(row)


class GetTransactionsFilter(BaseModel):
    account_ids: list[int] | None = None
    instrument_id: int | None = None
    from_date: str | None = None
    to_date: str | None = None
    txn_type: str | None = None
    flag_filter: str | None = None  # "all" | "flagged" | "clean"
    search: str | None = None
    sort_col: str | None = None
    sort_dir: str | None = None
    limit: int | None = None
    offset: int | None = None


class CreateTransactionInput(BaseModel):
    account_id: int
    instrument_id: int | None = None
    existing_pending_id: int | None = None
    pending_instrument: dict | None = None  # {name, type, metadata}
    txn_type: str
    trade_segment: str
    trade_date: str
    txn_time: str | None = None
    quantity: float
    effective_price_paise: int
    actual_price_paise: int | None = None
    brokerage_per_unit_paise: int | None = None
    stt_paise: int = 0
    other_charges_paise: int = 0
    notes: str | None = None
    broker_ref: str | None = None


class UpdateTransactionInput(BaseModel):
    txn_id: int
    txn_type: str
    trade_segment: str
    trade_date: str
    txn_time: str | None = None
    quantity: float
    effective_price_paise: int
    actual_price_paise: int | None = None
    brokerage_per_unit_paise: int | None = None
    stt_paise: int = 0
    other_charges_paise: int = 0
    notes: str | None = None


class TransferHoldingInput(BaseModel):
    from_account_id: int
    to_account_id: int
    instrument_id: int
    quantity: float
    effective_price_paise: int
    actual_price_paise: int | None = None
    trade_date: str
    trade_segment: str
    notes: str | None = None


class CreateSplitInput(BaseModel):
    account_id: int
    from_instrument_id: int
    to_instrument_id: int | None = None
    to_pending: dict | None = None
    qty_before: float
    qty_after: float
    avg_cost_before_paise: int
    trade_date: str
    notes: str | None = None


@router.post("/list")
def get_transactions(filter: GetTransactionsFilter, conn: Conn):
    where, params = _build_filter_sql(filter.model_dump())
    sql = _TXN_SELECT + " WHERE 1=1"
    if where:
        sql += " AND " + where

    sort_col = _SORT_COLS.get(filter.sort_col or "", "t.trade_date")
    sort_dir = "ASC" if filter.sort_dir == "asc" else "DESC"
    sql += f" ORDER BY {sort_col} {sort_dir}, t.txn_id DESC"

    if filter.limit is not None:
        sql += f" LIMIT {filter.limit} OFFSET {filter.offset or 0}"
    elif filter.offset is not None:
        sql += f" LIMIT -1 OFFSET {filter.offset}"

    rows = conn.execute(sql, params).fetchall()
    return [dict(r) for r in rows]


@router.post("/count")
def get_transactions_count(filter: GetTransactionsFilter, conn: Conn):
    where, params = _build_filter_sql(filter.model_dump())
    sql = """SELECT COUNT(*)
             FROM transactions t
             JOIN accounts a ON t.account_id = a.account_id
             LEFT JOIN instruments i ON i.instrument_id = t.instrument_id
             LEFT JOIN pending_instruments pi ON pi.pending_id = t.pending_instrument_id
             LEFT JOIN instrument_equity ie ON ie.instrument_id = t.instrument_id
             WHERE 1=1"""
    if where:
        sql += " AND " + where
    count = conn.execute(sql, params).fetchone()[0]
    return {"count": count}


@router.post("/flagged-count")
def get_flagged_count(body: dict, conn: Conn):
    account_ids = body.get("account_ids") or []
    sql = "SELECT COUNT(*) FROM transactions t WHERE t.flag IS NOT NULL AND t.flag_dismissed = 0"
    params = []
    if account_ids:
        placeholders = ",".join("?" * len(account_ids))
        sql += f" AND t.account_id IN ({placeholders})"
        params.extend(account_ids)
    count = conn.execute(sql, params).fetchone()[0]
    return {"count": count}


@router.get("/batch/{batch_id}")
def get_import_batch(batch_id: int, conn: Conn):
    row = conn.execute(
        """SELECT batch_id, account_id, source_type, file_name, ref_no, broker,
                  batch_trade_date, imported_at, record_count,
                  COALESCE(stt_paise,0), COALESCE(stamp_charges_paise,0),
                  COALESCE(gst_paise,0), COALESCE(trans_charges_paise,0),
                  COALESCE(other_charges_paise,0), COALESCE(total_payable_paise,0)
           FROM import_batches WHERE batch_id = ?""",
        (batch_id,),
    ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="Batch not found")

    batch = dict(row)
    txn_rows = conn.execute(
        _TXN_SELECT + " WHERE t.batch_id = ? ORDER BY t.trade_date, t.txn_time, t.txn_id",
        (batch_id,),
    ).fetchall()
    batch["transactions"] = [dict(r) for r in txn_rows]
    return batch


@router.post("")
def create_transaction(body: CreateTransactionInput, conn: Conn):
    # Resolve instrument
    instrument_id = None
    pending_instrument_id = None

    if body.instrument_id is not None:
        instrument_id = body.instrument_id
    elif body.existing_pending_id is not None:
        pending_instrument_id = body.existing_pending_id
    elif body.pending_instrument is not None:
        spec = body.pending_instrument
        cur = conn.execute(
            "INSERT INTO pending_instruments (name, type, metadata) VALUES (?, ?, ?)",
            (spec["name"], spec["type"], json.dumps(spec.get("metadata", {}))),
        )
        pending_instrument_id = cur.lastrowid
    else:
        raise HTTPException(status_code=400, detail="instrument_id, existing_pending_id, or pending_instrument required")

    cur = conn.execute(
        """INSERT INTO transactions
               (account_id, instrument_id, pending_instrument_id, txn_type, trade_segment,
                trade_date, txn_time, quantity,
                actual_price_paise, brokerage_per_unit_paise, effective_price_paise,
                stt_paise, other_charges_paise, notes, broker_ref)
           VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
        (body.account_id, instrument_id, pending_instrument_id, body.txn_type,
         body.trade_segment, body.trade_date, body.txn_time, body.quantity,
         body.actual_price_paise, body.brokerage_per_unit_paise, body.effective_price_paise,
         body.stt_paise, body.other_charges_paise, body.notes, body.broker_ref),
    )
    conn.commit()
    txn_id = cur.lastrowid

    flag_oversells(conn, body.account_id)
    return _get_txn(conn, txn_id)


@router.patch("")
def update_transaction(body: UpdateTransactionInput, conn: Conn):
    row = conn.execute(
        "SELECT account_id FROM transactions WHERE txn_id = ?", (body.txn_id,)
    ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="Transaction not found")
    account_id = row[0]

    conn.execute(
        """UPDATE transactions SET
               txn_type=?, trade_segment=?, trade_date=?, txn_time=?,
               quantity=?, actual_price_paise=?, brokerage_per_unit_paise=?,
               effective_price_paise=?, stt_paise=?, other_charges_paise=?, notes=?,
               flag=NULL, flag_reason=NULL, flag_dismissed=0
           WHERE txn_id=?""",
        (body.txn_type, body.trade_segment, body.trade_date, body.txn_time,
         body.quantity, body.actual_price_paise, body.brokerage_per_unit_paise,
         body.effective_price_paise, body.stt_paise, body.other_charges_paise,
         body.notes, body.txn_id),
    )
    conn.commit()

    flag_oversells(conn, account_id)
    return _get_txn(conn, body.txn_id)


@router.delete("/{txn_id}")
def delete_transaction(txn_id: int, conn: Conn):
    row = conn.execute(
        "SELECT account_id FROM transactions WHERE txn_id = ?", (txn_id,)
    ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="Transaction not found")
    account_id = row[0]

    conn.execute("DELETE FROM transactions WHERE txn_id = ?", (txn_id,))
    conn.commit()
    flag_oversells(conn, account_id)
    return {"ok": True}


@router.post("/{txn_id}/dismiss-flag")
def dismiss_transaction_flag(txn_id: int, conn: Conn):
    conn.execute(
        "UPDATE transactions SET flag_dismissed=1 WHERE txn_id=?", (txn_id,)
    )
    conn.commit()
    return {"ok": True}


@router.post("/re-evaluate-flags")
def re_evaluate_flags(body: dict, conn: Conn):
    account_id = body.get("account_id")
    re_evaluate_all(conn, account_id)
    return {"ok": True}


@router.post("/transfer")
def transfer_holding(body: TransferHoldingInput, conn: Conn):
    pair_ref = f"TRF-{int(time.time() * 1000)}"

    cur_out = conn.execute(
        """INSERT INTO transactions
               (account_id, instrument_id, txn_type, trade_segment, trade_date,
                quantity, actual_price_paise, effective_price_paise,
                stt_paise, other_charges_paise, notes, broker_ref)
           VALUES (?,?,'TRANSFER_OUT',?,?,?,?,?,0,0,?,?)""",
        (body.from_account_id, body.instrument_id, body.trade_segment, body.trade_date,
         body.quantity, body.actual_price_paise, body.effective_price_paise,
         body.notes, pair_ref),
    )
    out_id = cur_out.lastrowid

    cur_in = conn.execute(
        """INSERT INTO transactions
               (account_id, instrument_id, txn_type, trade_segment, trade_date,
                quantity, actual_price_paise, effective_price_paise,
                stt_paise, other_charges_paise, notes, broker_ref)
           VALUES (?,?,'TRANSFER_IN',?,?,?,?,?,0,0,?,?)""",
        (body.to_account_id, body.instrument_id, body.trade_segment, body.trade_date,
         body.quantity, body.actual_price_paise, body.effective_price_paise,
         body.notes, pair_ref),
    )
    in_id = cur_in.lastrowid

    conn.execute(
        "INSERT INTO corporate_actions (data) VALUES (?)",
        (json.dumps({
            "type": "TRANSFER", "transfer_date": body.trade_date,
            "from_account_id": body.from_account_id, "to_account_id": body.to_account_id,
            "instrument_id": body.instrument_id, "quantity": body.quantity,
            "effective_price_paise": body.effective_price_paise,
            "transfer_out_txn_id": out_id, "transfer_in_txn_id": in_id,
        }),),
    )
    conn.commit()

    flag_oversells(conn, body.from_account_id)
    flag_oversells(conn, body.to_account_id)
    return {"transfer_out_txn_id": out_id, "transfer_in_txn_id": in_id}


@router.post("/split")
def create_split(body: CreateSplitInput, conn: Conn):
    from_instr_id = body.from_instrument_id if body.from_instrument_id > 0 else None
    from_pending_id = -body.from_instrument_id if body.from_instrument_id < 0 else None

    to_instr_id = None
    to_pending_id = None
    if body.to_instrument_id is not None:
        to_instr_id = body.to_instrument_id
    elif body.to_pending is not None:
        spec = body.to_pending
        cur = conn.execute(
            "INSERT INTO pending_instruments (name, type, metadata) VALUES (?, ?, ?)",
            (spec["name"], spec["type"], json.dumps(spec.get("metadata", {}))),
        )
        to_pending_id = cur.lastrowid
    else:
        raise HTTPException(status_code=400, detail="to_instrument_id or to_pending required")

    price_after = int(body.avg_cost_before_paise * body.qty_before / body.qty_after) if body.qty_after > 0 else 0

    cur_out = conn.execute(
        """INSERT INTO transactions
               (account_id, instrument_id, pending_instrument_id,
                txn_type, trade_segment, trade_date,
                quantity, effective_price_paise, stt_paise, other_charges_paise, notes)
           VALUES (?,?,?,'SPLIT_OUT','DELIVERY',?,?,?,0,0,?)""",
        (body.account_id, from_instr_id, from_pending_id,
         body.trade_date, body.qty_before, body.avg_cost_before_paise, body.notes),
    )
    out_id = cur_out.lastrowid

    cur_in = conn.execute(
        """INSERT INTO transactions
               (account_id, instrument_id, pending_instrument_id,
                txn_type, trade_segment, trade_date,
                quantity, effective_price_paise, stt_paise, other_charges_paise, notes)
           VALUES (?,?,?,'SPLIT_IN','DELIVERY',?,?,?,0,0,?)""",
        (body.account_id, to_instr_id, to_pending_id,
         body.trade_date, body.qty_after, price_after, body.notes),
    )
    in_id = cur_in.lastrowid

    cur_ca = conn.execute(
        "INSERT INTO corporate_actions (data) VALUES (?)",
        (json.dumps({
            "type": "SPLIT", "ex_date": body.trade_date,
            "account_id": body.account_id,
            "from_instrument_id": body.from_instrument_id,
            "to_instrument_id": body.to_instrument_id,
            "qty_before": body.qty_before, "qty_after": body.qty_after,
            "avg_cost_before_paise": body.avg_cost_before_paise,
            "split_out_txn_id": out_id, "split_in_txn_id": in_id,
        }),),
    )
    ca_id = cur_ca.lastrowid
    conn.commit()

    flag_oversells(conn, body.account_id)
    return {"ca_id": ca_id, "split_out_txn_id": out_id, "split_in_txn_id": in_id}
