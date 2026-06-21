import sqlite3
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from routers.deps import get_conn

router = APIRouter(tags=["tax"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]

_VALID_TYPES = {"TDS", "ADVANCE_TAX", "SELF_ASSESSMENT_TAX"}


class TaxEntryInput(BaseModel):
    person_id: int
    entry_type: str
    amount_paise: int
    entry_date: str
    fy: str
    txn_id: int | None = None
    notes: str | None = None


def _row(conn: sqlite3.Connection, entry_id: int) -> dict:
    row = conn.execute(
        """SELECT e.entry_id, e.person_id, p.name AS person_name,
                  e.entry_type, e.amount_paise, e.entry_date,
                  e.fy, e.txn_id, e.notes, e.created_at
           FROM tax_entries e
           JOIN persons p ON p.person_id = e.person_id
           WHERE e.entry_id = ?""",
        (entry_id,),
    ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="Tax entry not found")
    return dict(row)


@router.post("/list")
def get_tax_entries(body: dict, conn: Conn):
    person_id = body.get("person_id")
    fy = body.get("fy")

    sql = """SELECT e.entry_id, e.person_id, p.name AS person_name,
                    e.entry_type, e.amount_paise, e.entry_date,
                    e.fy, e.txn_id, e.notes, e.created_at
             FROM tax_entries e
             JOIN persons p ON p.person_id = e.person_id
             WHERE 1=1"""
    params = []
    if person_id is not None:
        sql += " AND e.person_id = ?"
        params.append(person_id)
    if fy is not None:
        sql += " AND e.fy = ?"
        params.append(fy)
    sql += " ORDER BY e.entry_date DESC, e.entry_id DESC"

    rows = conn.execute(sql, params).fetchall()
    return [dict(r) for r in rows]


@router.post("")
def create_tax_entry(body: TaxEntryInput, conn: Conn):
    if body.entry_type not in _VALID_TYPES:
        raise HTTPException(status_code=400, detail=f"Invalid entry_type: {body.entry_type}")
    if body.amount_paise <= 0:
        raise HTTPException(status_code=400, detail="Amount must be greater than zero")
    if not body.entry_date:
        raise HTTPException(status_code=400, detail="Entry date is required")
    if not body.fy:
        raise HTTPException(status_code=400, detail="Financial year is required")
    cur = conn.execute(
        """INSERT INTO tax_entries (person_id, entry_type, amount_paise, entry_date,
                                    fy, txn_id, notes)
           VALUES (?, ?, ?, ?, ?, ?, ?)""",
        (body.person_id, body.entry_type, body.amount_paise, body.entry_date,
         body.fy, body.txn_id, body.notes),
    )
    conn.commit()
    return _row(conn, cur.lastrowid)


@router.delete("/{entry_id}")
def delete_tax_entry(entry_id: int, conn: Conn):
    _row(conn, entry_id)
    conn.execute("DELETE FROM tax_entries WHERE entry_id = ?", (entry_id,))
    conn.commit()
    return {"ok": True}
