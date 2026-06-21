import sqlite3
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from routers.deps import get_conn

router = APIRouter(tags=["portfolios"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]


class CreatePortfolioInput(BaseModel):
    name: str
    person_id: int | None = None


def _row(conn: Conn, portfolio_id: int) -> dict:
    row = conn.execute(
        "SELECT portfolio_id, person_id, name, created_at FROM portfolios WHERE portfolio_id = ?",
        (portfolio_id,),
    ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="Portfolio not found")
    return dict(row)


@router.get("")
def get_portfolios(conn: Conn):
    rows = conn.execute(
        "SELECT portfolio_id, person_id, name, created_at FROM portfolios ORDER BY portfolio_id"
    ).fetchall()
    return [dict(r) for r in rows]


@router.post("")
def create_portfolio(body: CreatePortfolioInput, conn: Conn):
    cur = conn.execute(
        "INSERT INTO portfolios (name, person_id) VALUES (?, ?)",
        (body.name, body.person_id),
    )
    conn.commit()
    return _row(conn, cur.lastrowid)


@router.patch("/{portfolio_id}/rename")
def rename_portfolio(portfolio_id: int, body: dict, conn: Conn):
    name = body.get("name", "").strip()
    if not name:
        raise HTTPException(status_code=400, detail="Name is required")
    _row(conn, portfolio_id)  # 404 check
    conn.execute(
        "UPDATE portfolios SET name = ? WHERE portfolio_id = ?",
        (name, portfolio_id),
    )
    conn.commit()
    return _row(conn, portfolio_id)


@router.delete("/{portfolio_id}")
def delete_portfolio(portfolio_id: int, conn: Conn):
    _row(conn, portfolio_id)  # 404 check
    has_accounts = conn.execute(
        "SELECT 1 FROM accounts WHERE portfolio_id = ? LIMIT 1", (portfolio_id,)
    ).fetchone()
    if has_accounts:
        raise HTTPException(status_code=400, detail="Cannot delete portfolio with accounts")
    conn.execute("DELETE FROM portfolios WHERE portfolio_id = ?", (portfolio_id,))
    conn.commit()
    return {"ok": True}
