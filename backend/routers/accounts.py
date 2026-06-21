import sqlite3
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from routers.deps import get_conn

router = APIRouter(tags=["accounts"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]


class CreateAccountInput(BaseModel):
    portfolio_id: int
    name: str
    account_type: str
    broker: str | None = None
    account_no: str | None = None


def _row(conn: Conn, account_id: int) -> dict:
    row = conn.execute(
        """SELECT account_id, portfolio_id, name, account_type, broker, account_no, created_at
           FROM accounts WHERE account_id = ?""",
        (account_id,),
    ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="Account not found")
    return dict(row)


@router.get("")
def get_accounts(portfolio_id: int | None = None, conn: sqlite3.Connection = Depends(get_conn)):
    if portfolio_id is not None:
        rows = conn.execute(
            """SELECT account_id, portfolio_id, name, account_type, broker, account_no, created_at
               FROM accounts WHERE portfolio_id = ? ORDER BY account_id""",
            (portfolio_id,),
        ).fetchall()
    else:
        rows = conn.execute(
            """SELECT account_id, portfolio_id, name, account_type, broker, account_no, created_at
               FROM accounts ORDER BY account_id"""
        ).fetchall()
    return [dict(r) for r in rows]


@router.post("")
def create_account(body: CreateAccountInput, conn: Conn):
    cur = conn.execute(
        """INSERT INTO accounts (portfolio_id, name, account_type, broker, account_no)
           VALUES (?, ?, ?, ?, ?)""",
        (body.portfolio_id, body.name, body.account_type, body.broker, body.account_no),
    )
    conn.commit()
    return _row(conn, cur.lastrowid)


@router.patch("/{account_id}/rename")
def rename_account(account_id: int, body: dict, conn: Conn):
    name = body.get("name", "").strip()
    if not name:
        raise HTTPException(status_code=400, detail="Name is required")
    _row(conn, account_id)
    conn.execute("UPDATE accounts SET name = ? WHERE account_id = ?", (name, account_id))
    conn.commit()
    return _row(conn, account_id)


@router.delete("/{account_id}")
def delete_account(account_id: int, conn: Conn):
    _row(conn, account_id)
    has_txns = conn.execute(
        "SELECT 1 FROM transactions WHERE account_id = ? LIMIT 1", (account_id,)
    ).fetchone()
    if has_txns:
        raise HTTPException(status_code=400, detail="Cannot delete account with transactions")
    conn.execute("DELETE FROM accounts WHERE account_id = ?", (account_id,))
    conn.commit()
    return {"ok": True}
