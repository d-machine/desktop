import sqlite3
from typing import Annotated

from fastapi import APIRouter, Depends, HTTPException
from pydantic import BaseModel

from routers.deps import get_conn

router = APIRouter(tags=["persons"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]


class CreatePersonInput(BaseModel):
    name: str
    pan: str | None = None


class UpdatePersonInput(BaseModel):
    name: str | None = None
    pan: str | None = None


def _row(conn: Conn, person_id: int) -> dict:
    row = conn.execute(
        "SELECT person_id, name, pan, created_at FROM persons WHERE person_id = ?",
        (person_id,),
    ).fetchone()
    if row is None:
        raise HTTPException(status_code=404, detail="Person not found")
    return dict(row)


@router.get("")
def get_persons(conn: Conn):
    rows = conn.execute(
        "SELECT person_id, name, pan, created_at FROM persons ORDER BY person_id"
    ).fetchall()
    return [dict(r) for r in rows]


@router.post("")
def create_person(body: CreatePersonInput, conn: Conn):
    cur = conn.execute(
        "INSERT INTO persons (name, pan) VALUES (?, ?)",
        (body.name, body.pan or None),
    )
    conn.commit()
    return _row(conn, cur.lastrowid)


@router.patch("/{person_id}")
def update_person(person_id: int, body: UpdatePersonInput, conn: Conn):
    existing = _row(conn, person_id)
    new_name = body.name if body.name is not None else existing["name"]
    # Empty string for pan means clear it
    if body.pan is None:
        new_pan = existing["pan"]
    elif body.pan == "":
        new_pan = None
    else:
        new_pan = body.pan

    conn.execute(
        "UPDATE persons SET name = ?, pan = ? WHERE person_id = ?",
        (new_name, new_pan, person_id),
    )
    conn.commit()
    return _row(conn, person_id)
