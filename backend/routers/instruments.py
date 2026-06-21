import json
import sqlite3
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from routers.deps import get_conn
from services.flags import flag_oversells

router = APIRouter(tags=["instruments"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]


class CreateInstrumentInput(BaseModel):
    isin: str | None = None
    name: str
    instrument_type_id: int
    primary_exchange_id: int | None = None
    nse_symbol: str | None = None
    bse_code: str | None = None
    amfi_code: str | None = None


class UpdatePendingInput(BaseModel):
    name: str
    metadata: dict


class ResolvePendingInput(BaseModel):
    instrument_id: int


@router.get("/search")
def search_instruments(q: str, conn: Conn):
    query = q.strip()
    q_upper = query.upper()
    q_like = f"%{query}%"

    rows = conn.execute(
        """
        SELECT i.instrument_id,
               ie.isin,
               i.name,
               it.asset_class,
               e.code     AS exchange_code,
               ie.nse_symbol,
               im.amfi_code,
               NULL       AS pending_instrument_id,
               NULL       AS pending_metadata
        FROM instruments i
        JOIN instrument_types it ON i.instrument_type_id = it.instrument_type_id
        LEFT JOIN exchanges e ON i.primary_exchange_id = e.exchange_id
        LEFT JOIN instrument_equity ie ON ie.instrument_id = i.instrument_id
        LEFT JOIN instrument_mf     im ON im.instrument_id = i.instrument_id
        WHERE ie.isin       = ?
           OR ie.nse_symbol LIKE ?
           OR ie.bse_code   = ?
           OR im.amfi_code  = ?
           OR i.name        LIKE ?

        UNION ALL

        SELECT -p.pending_id,
               NULL, p.name, p.type,
               NULL, NULL, NULL,
               p.pending_id, p.metadata
        FROM pending_instruments p
        WHERE p.name LIKE ?

        ORDER BY name
        LIMIT 20
        """,
        (q_upper, q_like, q_upper, q_upper, q_like, q_like),
    ).fetchall()

    return [dict(r) for r in rows]


@router.get("/types")
def get_instrument_types(conn: Conn):
    rows = conn.execute(
        "SELECT instrument_type_id, name, asset_class FROM instrument_types ORDER BY name"
    ).fetchall()
    return [dict(r) for r in rows]


@router.post("")
def create_instrument(body: CreateInstrumentInput, conn: Conn):
    cur = conn.execute(
        "INSERT INTO instruments (name, instrument_type_id, primary_exchange_id, source) VALUES (?, ?, ?, 'MANUAL')",
        (body.name, body.instrument_type_id, body.primary_exchange_id),
    )
    instrument_id = cur.lastrowid

    if body.isin or body.nse_symbol or body.bse_code:
        conn.execute(
            "INSERT INTO instrument_equity (instrument_id, isin, nse_symbol, bse_code) VALUES (?, ?, ?, ?)",
            (instrument_id, body.isin, body.nse_symbol, body.bse_code),
        )

    if body.amfi_code:
        conn.execute(
            "INSERT INTO instrument_mf (instrument_id, amfi_code) VALUES (?, ?)",
            (instrument_id, body.amfi_code),
        )

    conn.commit()
    return {
        "instrument_id": instrument_id,
        "isin": body.isin,
        "name": body.name,
        "asset_class": "",
        "exchange_code": None,
        "nse_symbol": body.nse_symbol,
        "amfi_code": body.amfi_code,
        "pending_instrument_id": None,
        "pending_metadata": None,
    }


@router.get("/pending")
def list_pending_instruments(conn: Conn):
    rows = conn.execute(
        """SELECT p.pending_id, p.name, p.type, p.metadata,
                  COUNT(t.txn_id) AS txn_count,
                  MIN(t.trade_date) AS earliest_date,
                  GROUP_CONCAT(DISTINCT t.account_id) AS account_ids
           FROM pending_instruments p
           JOIN transactions t ON t.pending_instrument_id = p.pending_id
           GROUP BY p.pending_id
           ORDER BY p.name"""
    ).fetchall()
    return [dict(r) for r in rows]


@router.patch("/pending/{pending_id}")
def update_pending_instrument(pending_id: int, body: UpdatePendingInput, conn: Conn):
    conn.execute(
        "UPDATE pending_instruments SET name = ?, metadata = ? WHERE pending_id = ?",
        (body.name, json.dumps(body.metadata), pending_id),
    )
    conn.commit()
    return {"ok": True}


@router.post("/pending/{pending_id}/resolve")
def resolve_pending_to_instrument(pending_id: int, body: ResolvePendingInput, conn: Conn):
    if not conn.execute(
        "SELECT 1 FROM instruments WHERE instrument_id = ?", (body.instrument_id,)
    ).fetchone():
        raise HTTPException(status_code=404, detail="Target instrument not found")

    account_ids = [
        r[0] for r in conn.execute(
            "SELECT DISTINCT account_id FROM transactions WHERE pending_instrument_id = ?",
            (pending_id,),
        ).fetchall()
    ]

    cur = conn.execute(
        "UPDATE transactions SET instrument_id = ?, pending_instrument_id = NULL WHERE pending_instrument_id = ?",
        (body.instrument_id, pending_id),
    )
    migrated = cur.rowcount

    conn.execute("DELETE FROM pending_instruments WHERE pending_id = ?", (pending_id,))
    conn.commit()

    for aid in account_ids:
        flag_oversells(conn, aid)

    return {"ok": True, "migrated_txns": migrated}
