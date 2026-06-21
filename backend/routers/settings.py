import sqlite3
from typing import Annotated

from fastapi import APIRouter, Depends
from pydantic import BaseModel

from routers.deps import get_conn

router = APIRouter(tags=["settings"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]


class SetSettingInput(BaseModel):
    key: str
    value: str


@router.get("/{key}")
def get_setting(key: str, conn: Conn):
    row = conn.execute(
        "SELECT value FROM app_settings WHERE key = ?", (key,)
    ).fetchone()
    return {"key": key, "value": row["value"] if row else None}


@router.post("")
def set_setting(body: SetSettingInput, conn: Conn):
    conn.execute(
        """INSERT INTO app_settings (key, value, updated_at)
           VALUES (?, ?, datetime('now'))
           ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at""",
        (body.key, body.value),
    )
    conn.commit()
    return {"ok": True}
