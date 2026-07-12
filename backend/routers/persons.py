import sqlite3
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException

from routers.deps import get_conn

router = APIRouter(tags=["persons"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]


def _row(conn: sqlite3.Connection, person_id: int) -> dict:
    row = conn.execute(
        """SELECT person_id, name, masked_pan, pan_hash, display_name,
                  subscription_status, subscription_expires_at, paid_price, created_at
           FROM persons WHERE person_id = ?""",
        (person_id,),
    ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="Person not found")
    return dict(row)


@router.get("")
def get_persons(conn: Conn):
    rows = conn.execute(
        """SELECT person_id, name, masked_pan, pan_hash, display_name,
                  subscription_status, subscription_expires_at, paid_price, created_at
           FROM persons ORDER BY person_id"""
    ).fetchall()
    return [dict(r) for r in rows]
