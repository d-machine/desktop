import argparse
import os
import sqlite3
import sys

# When bundled as a windowless PyInstaller exe, sys.stdout/stderr are None.
# Redirect to devnull so uvicorn's logging formatters don't crash on isatty().
if sys.stdout is None:
    sys.stdout = open(os.devnull, "w")
if sys.stderr is None:
    sys.stderr = open(os.devnull, "w")
from contextlib import asynccontextmanager
from pathlib import Path

import uvicorn
from fastapi import FastAPI
from fastapi.middleware.cors import CORSMiddleware

from core import db as db_module
from core import session as session_module
from routers import (
    accounts,
    auth,
    backup,
    charges,
    holdings,
    import_,
    instruments,
    persons,
    portfolios,
    prices,
    reports,
    server_auth,
    settings,
    tax,
    transactions,
)

# ---------------------------------------------------------------------------
# CLI args — parsed once at module load so lifespan can read them.
# ---------------------------------------------------------------------------
_parser = argparse.ArgumentParser(description="Arthdesk FastAPI backend")
_parser.add_argument("--port", type=int, default=8742)
_parser.add_argument("--db-path", type=str, required=True, help="App data directory path")
_parser.add_argument("--server-url", type=str, default="", help="Override server URL in app_settings on every startup")
_args = _parser.parse_args()

APP_DIR = Path(_args.db_path)
APP_DIR.mkdir(parents=True, exist_ok=True)


# ---------------------------------------------------------------------------
# Lifespan — runs on startup and shutdown.
# ---------------------------------------------------------------------------
@asynccontextmanager
async def lifespan(app: FastAPI):
    app.state.app_dir = APP_DIR
    app.state.db_path = APP_DIR / "portfolio.db"
    app.state.conn = None  # set by POST /api/auth/login or /api/auth/setup
    app.state.default_server_url = _args.server_url

    yield

    # Clean shutdown: if a session is active, close and encrypt the DB.
    conn: sqlite3.Connection | None = app.state.conn
    if conn is not None:
        master_key = session_module.get_active_key()
        if master_key is not None:
            db_module.close_database(APP_DIR, master_key, conn)
        else:
            conn.close()
        app.state.conn = None

    session_module.destroy_all()


# ---------------------------------------------------------------------------
# App
# ---------------------------------------------------------------------------
app = FastAPI(title="Arthdesk Backend", lifespan=lifespan)

app.add_middleware(
    CORSMiddleware,
    allow_origins=["*"],
    allow_methods=["*"],
    allow_headers=["*"],
)

app.include_router(auth.router,        prefix="/api/auth")
app.include_router(persons.router,     prefix="/api/persons")
app.include_router(portfolios.router,  prefix="/api/portfolios")
app.include_router(accounts.router,    prefix="/api/accounts")
app.include_router(instruments.router, prefix="/api/instruments")
app.include_router(transactions.router,prefix="/api/transactions")
app.include_router(holdings.router,    prefix="/api/holdings")
app.include_router(prices.router,      prefix="/api/prices")
app.include_router(reports.router,     prefix="/api/reports")
app.include_router(charges.router,     prefix="/api/charges")
app.include_router(tax.router,         prefix="/api/tax")
app.include_router(import_.router,     prefix="/api/import")
app.include_router(backup.router,      prefix="/api/backup")
app.include_router(settings.router,    prefix="/api/settings")
app.include_router(server_auth.router, prefix="/api/server-auth")


@app.get("/health")
def health():
    return {"ok": True}


# ---------------------------------------------------------------------------
# Entry point
# ---------------------------------------------------------------------------
if __name__ == "__main__":
    reload = os.environ.get("ARTHDESK_RELOAD", "0") == "1"
    uvicorn.run(
        "main:app" if reload else app,
        host="127.0.0.1",
        port=_args.port,
        log_level="info",
        reload=reload,
        reload_dirs=["./"] if reload else None,
    )
