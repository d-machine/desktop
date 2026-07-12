import sqlite3
from pathlib import Path

from fastapi import APIRouter, HTTPException, Request
from pydantic import BaseModel

from core import auth as auth_module
from core import db as db_module
from core import session as session_module

router = APIRouter(tags=["auth"])


# ---------------------------------------------------------------------------
# Helpers
# ---------------------------------------------------------------------------

def _app_dir(request: Request) -> Path:
    return request.app.state.app_dir


def _require_session(request: Request) -> tuple[str, bytes]:
    token = request.headers.get("X-Session-Token")
    if not token:
        raise HTTPException(status_code=401, detail="Missing X-Session-Token header")
    try:
        key = session_module.get_key(token)
    except KeyError:
        raise HTTPException(status_code=401, detail="Invalid or expired session token")
    return token, key


def _open_db(request: Request, master_key: bytes) -> None:
    if request.app.state.conn is None:
        server_url = getattr(request.app.state, "default_server_url", "")
        conn = db_module.open_database(_app_dir(request), master_key, default_server_url=server_url)
        request.app.state.conn = conn


# ---------------------------------------------------------------------------
# Models
# ---------------------------------------------------------------------------

class SetupInput(BaseModel):
    pin: str
    passphrase: str


class LoginInput(BaseModel):
    pin: str


class RecoverInput(BaseModel):
    recovery_json: str
    passphrase: str
    new_pin: str


class ChangePinInput(BaseModel):
    current_pin: str
    new_pin: str


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------

@router.get("/status")
def status(request: Request):
    app_dir = _app_dir(request)
    return {
        "setup": auth_module.is_setup(app_dir),
        "locked": not session_module.has_active_session(),
    }


@router.post("/setup")
def setup(body: SetupInput, request: Request):
    app_dir = _app_dir(request)
    try:
        recovery_json = auth_module.setup(app_dir, body.pin, body.passphrase)
    except ValueError as e:
        raise HTTPException(status_code=400, detail=str(e))

    master_key = auth_module.login(app_dir, body.pin)
    _open_db(request, master_key)
    token = session_module.create_session(master_key)
    return {"session_token": token, "recovery_json": recovery_json}


@router.post("/login")
def login(body: LoginInput, request: Request):
    app_dir = _app_dir(request)
    try:
        master_key = auth_module.login(app_dir, body.pin)
    except ValueError:
        raise HTTPException(status_code=401, detail="Wrong PIN")

    _open_db(request, master_key)
    token = session_module.create_session(master_key)
    return {"session_token": token}


@router.post("/lock")
def lock(request: Request):
    token, master_key = _require_session(request)
    conn: sqlite3.Connection | None = request.app.state.conn
    if conn is not None:
        db_module.close_database(_app_dir(request), master_key, conn)
        request.app.state.conn = None
    session_module.destroy_session(token)
    return {"ok": True}


@router.post("/recover")
def recover(body: RecoverInput, request: Request):
    app_dir = _app_dir(request)
    try:
        master_key = auth_module.recover(
            app_dir, body.recovery_json, body.passphrase, body.new_pin
        )
    except (ValueError, KeyError) as e:
        raise HTTPException(status_code=401, detail=str(e))

    _open_db(request, master_key)
    token = session_module.create_session(master_key)
    return {"session_token": token}


@router.post("/change-pin")
def change_pin(body: ChangePinInput, request: Request):
    token, master_key = _require_session(request)
    app_dir = _app_dir(request)
    try:
        auth_module.change_pin(app_dir, master_key, body.current_pin, body.new_pin)
    except ValueError as e:
        raise HTTPException(status_code=401, detail=str(e))
    return {"ok": True}
