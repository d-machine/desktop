"""Shared FastAPI dependencies used by all routers."""
import sqlite3
from pathlib import Path
from typing import Generator

from fastapi import Header, HTTPException, Request

from core import session as session_module


def get_conn(
    request: Request,
    x_session_token: str = Header(alias="X-Session-Token", default=None),
) -> Generator[sqlite3.Connection, None, None]:
    if not x_session_token:
        raise HTTPException(status_code=401, detail="Missing X-Session-Token header")
    try:
        session_module.get_key(x_session_token)
    except KeyError:
        raise HTTPException(status_code=401, detail="Invalid or expired session token")

    db_path: Path | None = getattr(request.app.state, "db_path", None)
    if db_path is None or not db_path.exists():
        raise HTTPException(status_code=503, detail="Database not open — please login first")

    conn = sqlite3.connect(str(db_path), check_same_thread=False)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA journal_mode=WAL")
    conn.execute("PRAGMA foreign_keys=ON")
    conn.execute("PRAGMA busy_timeout=5000")
    try:
        yield conn
    finally:
        conn.close()
