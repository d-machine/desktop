import sqlite3
import logging
from datetime import datetime, timezone
from pathlib import Path
from typing import Annotated

from fastapi import APIRouter, BackgroundTasks, Depends, HTTPException, Request
from pydantic import BaseModel

from routers.deps import get_conn
from services.flags import flag_oversells
from services import prices as prices_service

router = APIRouter(tags=["import"])
Conn = Annotated[sqlite3.Connection, Depends(get_conn)]
logger = logging.getLogger(__name__)

_SOURCES = [
    {"value": "BAJAJ_FINANCE",   "label": "Bajaj Financial Securities — Contract Note", "description": ".pdf Contract Note from Bajaj Broking (password: PAN)"},
    {"value": "ANGELONE",        "label": "Angel One — Trades & Charges",           "description": ".xlsx from Angel One back-office"},
    {"value": "CAMS_CAS",        "label": "CAMS — Consolidated Account Statement",  "description": ".pdf CAS from mycams.com (all AMCs)"},
    {"value": "CHOICE_MF",       "label": "Choice Wealth — MF Statement",           "description": ".pdf from Choice Wealth MF portal"},
    {"value": "CE_GLOBAL",       "label": "Choice Equity Global",                   "description": ".pdf Global Details Report from Choice Equity"},
    {"value": "ICICI_EQUITY",    "label": "ICICI Securities — Equity TRX",          "description": ".pdf TRX-Equity statement from ICICI Securities"},
    {"value": "CN_CHOICE_EQUITY","label": "Choice Equity — Contract Note",          "description": ".pdf Contract Note from Choice Equity Broking"},
    {"value": "CN_WOODSTOCK",    "label": "Woodstock Broking — Contract Note",      "description": ".pdf Contract Note from Woodstock Broking"},
    {"value": "CN_NIRMAL_BANG",  "label": "Nirmal Bang — Contract Note",            "description": ".pdf Contract Note from Nirmal Bang Securities"},
    {"value": "INVEST_PLUS_OPENING_STOCK", "label": "Invest Plus — Opening Stock Report", "description": ".xls Opening Stock Report from Invest Plus (financial year end)"},
]

_PARSER_MAP = {
    "BAJAJ_FINANCE":    "importers.bajaj_finance",
    "ANGELONE":         "importers.angel_one",
    "CAMS_CAS":         "importers.cams_cas",
    "CHOICE_MF":        "importers.choice_mf",
    "CE_GLOBAL":        "importers.ce_global",
    "ICICI_EQUITY":     "importers.icici_equity",
    "CN_CHOICE_EQUITY": "importers.cn_choice_equity",
    "CN_WOODSTOCK":     "importers.cn_woodstock",
    "CN_NIRMAL_BANG":              "importers.cn_nirmal_bang",
    "INVEST_PLUS_OPENING_STOCK":   "importers.invest_plus_opening_stock",
}


def _get_parser(source: str):
    module_name = _PARSER_MAP.get(source)
    if not module_name:
        raise HTTPException(status_code=400, detail=f"Unknown import source: {source}")
    import importlib
    return importlib.import_module(module_name)


class ParseInput(BaseModel):
    source: str
    file_path: str
    account_id: int | None = None   # used to auto-try password per account+source or PAN
    password: str | None = None


class ImportPasswordInput(BaseModel):
    source: str
    account_id: int
    password: str


def _ensure_import_passwords_table(conn: sqlite3.Connection) -> None:
    # Use execute (not executescript) to avoid implicit COMMIT that corrupts
    # the connection's transaction state for subsequent parameterized queries.
    conn.execute(
        "CREATE TABLE IF NOT EXISTS import_passwords "
        "(account_id INTEGER NOT NULL REFERENCES accounts(account_id), "
        "source TEXT NOT NULL, password TEXT NOT NULL, "
        "updated_at TEXT NOT NULL DEFAULT (datetime('now')), "
        "PRIMARY KEY (account_id, source))"
    )
    conn.commit()


class ImportInput(BaseModel):
    source: str
    account_id: int
    data: dict
    file_name: str | None = None


def _sync_after_import(db_path: str) -> None:
    """Best-effort enrichment after imports create pending instruments."""
    conn = sqlite3.connect(db_path, check_same_thread=False)
    conn.row_factory = sqlite3.Row
    conn.execute("PRAGMA foreign_keys=ON")
    try:
        prices_service.resolve_instruments(conn)
        prices_service.sync_prices(conn, force=True)
        conn.commit()
    except Exception as exc:
        logger.warning("Post-import instrument/price sync failed: %s", exc)
    finally:
        conn.close()


@router.get("/sources")
def get_import_sources():
    return _SOURCES


@router.post("/parse")
def parse_statement(body: ParseInput, conn: Conn):
    _ensure_import_passwords_table(conn)
    parser = _get_parser(body.source)

    password = body.password
    saved_password = None

    if password is None and body.account_id is not None:
        row = conn.execute(
            "SELECT password FROM import_passwords WHERE account_id = ? AND source = ?",
            (body.account_id, body.source),
        ).fetchone()
        if row and row[0]:
            saved_password = row[0]
            password = saved_password

    # If no saved password exists, auto-try the account owner's PAN
    if password is None and body.account_id is not None:
        row = conn.execute(
            """SELECT pe.pan FROM accounts a
               JOIN portfolios po ON a.portfolio_id = po.portfolio_id
               JOIN persons pe ON po.person_id = pe.person_id
               WHERE a.account_id = ? AND pe.pan IS NOT NULL""",
            (body.account_id,),
        ).fetchone()
        if row and row[0]:
            password = row[0]

    try:
        result = parser.parse(body.file_path, password=password)
    except NotImplementedError as e:
        raise HTTPException(status_code=501, detail=str(e))
    except Exception as e:
        err_str = str(e)
        # Detect password-related failures — PdfminerException str() is ".." when encrypted
        is_pdf_password_err = (
            err_str in ("..", "")
            or "PDFPasswordIncorrect" in type(e).__name__
            or "password" in err_str.lower()
            or "Wrong PDF password" in err_str
            or "decrypt" in err_str.lower()
        )
        if is_pdf_password_err:
            # If caller explicitly supplied a password and it failed, tell them it's wrong
            if body.password is not None:
                raise HTTPException(status_code=422, detail="WRONG_PASSWORD")
            # No password supplied (or auto-tried PAN/saved) — ask frontend to prompt
            raise HTTPException(status_code=422, detail="WRONG_PASSWORD")
        raise HTTPException(status_code=422, detail=f"Parse error: {e}")
    return result


@router.post("/password")
def save_import_password(body: ImportPasswordInput, conn: Conn):
    _ensure_import_passwords_table(conn)
    conn.execute(
        """INSERT INTO import_passwords (account_id, source, password, updated_at)
           VALUES (?, ?, ?, datetime('now'))
           ON CONFLICT(account_id, source) DO UPDATE SET
             password = excluded.password,
             updated_at = excluded.updated_at""",
        (body.account_id, body.source, body.password),
    )
    conn.commit()
    return {"ok": True}


@router.post("/confirm")
def import_statement(request: Request, body: ImportInput, background_tasks: BackgroundTasks, conn: Conn):
    parser = _get_parser(body.source)

    # Create import_batch record first
    now = datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S")
    cur = conn.execute(
        """INSERT INTO import_batches
               (account_id, source_type, file_name, imported_at, status)
           VALUES (?, ?, ?, ?, 'COMPLETED')""",
        (body.account_id, body.source, body.file_name, now),
    )
    batch_id = cur.lastrowid
    conn.commit()

    try:
        result = parser.import_(conn, body.account_id, body.data, batch_id=batch_id)
    except NotImplementedError as e:
        conn.execute("DELETE FROM import_batches WHERE batch_id=?", (batch_id,))
        conn.commit()
        raise HTTPException(status_code=501, detail=str(e))
    except Exception as e:
        conn.execute("DELETE FROM import_batches WHERE batch_id=?", (batch_id,))
        conn.commit()
        raise HTTPException(status_code=422, detail=f"Import error: {e}")

    # Update record count on batch
    count = result.get("imported", 0)
    conn.execute(
        "UPDATE import_batches SET record_count=? WHERE batch_id=?",
        (count, batch_id),
    )
    conn.commit()

    # Run oversell detection
    flag_oversells(conn, body.account_id)

    background_tasks.add_task(_sync_after_import, str(request.app.state.db_path))

    return {"batch_id": batch_id, **result}
