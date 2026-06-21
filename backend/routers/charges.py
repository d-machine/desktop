import sqlite3
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from routers.deps import get_conn

router = APIRouter(tags=["charges"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]


class ChargeInput(BaseModel):
    account_id: int
    start_date: str
    end_date: str
    charge_type: str
    amount_paise: int
    notes: str | None = None


def _row(conn: sqlite3.Connection, charge_id: int) -> dict:
    row = conn.execute(
        """SELECT charge_id, account_id, start_date, end_date, charge_type,
                  amount_paise, source, import_batch_id, notes, created_at
           FROM charges WHERE charge_id = ?""",
        (charge_id,),
    ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="Charge not found")
    return dict(row)


@router.post("/list")
def get_charges(body: dict, conn: Conn):
    account_ids = body.get("account_ids") or []
    if account_ids:
        placeholders = ",".join("?" * len(account_ids))
        rows = conn.execute(
            f"""SELECT charge_id, account_id, start_date, end_date, charge_type,
                       amount_paise, source, import_batch_id, notes, created_at
                FROM charges WHERE account_id IN ({placeholders})
                ORDER BY start_date DESC, charge_id DESC""",
            account_ids,
        ).fetchall()
    else:
        rows = conn.execute(
            """SELECT charge_id, account_id, start_date, end_date, charge_type,
                      amount_paise, source, import_batch_id, notes, created_at
               FROM charges ORDER BY start_date DESC, charge_id DESC"""
        ).fetchall()
    return [dict(r) for r in rows]


@router.post("")
def create_charge(body: ChargeInput, conn: Conn):
    cur = conn.execute(
        """INSERT INTO charges (account_id, start_date, end_date, charge_type,
                               amount_paise, source, notes)
           VALUES (?, ?, ?, ?, ?, 'MANUAL', ?)""",
        (body.account_id, body.start_date, body.end_date, body.charge_type,
         body.amount_paise, body.notes),
    )
    conn.commit()
    return _row(conn, cur.lastrowid)


@router.put("/{charge_id}")
def update_charge(charge_id: int, body: ChargeInput, conn: Conn):
    _row(conn, charge_id)
    conn.execute(
        """UPDATE charges SET start_date=?, end_date=?, charge_type=?,
                              amount_paise=?, notes=?
           WHERE charge_id=?""",
        (body.start_date, body.end_date, body.charge_type,
         body.amount_paise, body.notes, charge_id),
    )
    conn.commit()
    return _row(conn, charge_id)


@router.delete("/{charge_id}")
def delete_charge(charge_id: int, conn: Conn):
    _row(conn, charge_id)
    conn.execute("DELETE FROM charges WHERE charge_id = ?", (charge_id,))
    conn.commit()
    return {"ok": True}
