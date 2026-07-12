"""
Server authentication router.
Manages the remote server JWT tokens stored in app_settings.
These are layered on top of the local PIN auth — the app works offline without them.
"""
import json
import sqlite3
from datetime import datetime, timezone

import httpx
from fastapi import APIRouter, Depends, HTTPException, Request
from pydantic import BaseModel, EmailStr

from core.crypto import decrypt_server_response, ensure_keypair
from routers.deps import get_conn
from services.prices import _get_setting, _server_url

router = APIRouter()

_SERVER_TOKEN_KEYS = [
    "server_access_token",
    "server_refresh_token",
    "server_token_expires_at",
    "server_user_email",
    "server_subscription_status",
    "server_subscription_expires_at",
]


def _set_setting(conn: sqlite3.Connection, key: str, value: str) -> None:
    conn.execute(
        "INSERT OR REPLACE INTO app_settings (key, value) VALUES (?, ?)", (key, value)
    )
    conn.commit()


def _store_tokens(conn: sqlite3.Connection, data: dict, email: str = "") -> None:
    _set_setting(conn, "server_access_token",  data.get("access_token", ""))
    _set_setting(conn, "server_refresh_token", data.get("refresh_token", ""))

    sub = data.get("subscription") or {}
    _set_setting(conn, "server_subscription_status",     sub.get("status", ""))
    _set_setting(conn, "server_subscription_expires_at", sub.get("expires_at", "") or "")
    if email:
        _set_setting(conn, "server_user_email", email)


def _clear_tokens(conn: sqlite3.Connection) -> None:
    for key in _SERVER_TOKEN_KEYS:
        _set_setting(conn, key, "")


# ---------------------------------------------------------------------------
# Request models
# ---------------------------------------------------------------------------

class LoginRequest(BaseModel):
    email: EmailStr
    password: str


class ForgotPasswordRequest(BaseModel):
    email: EmailStr


_DEFAULT_SERVER_URL = "https://arthdeskapi.ashokitservices.com"


# ---------------------------------------------------------------------------
# Endpoints
# ---------------------------------------------------------------------------

@router.post("/validate")
def server_auth_validate(req: LoginRequest, request: Request):
    """
    Validate credentials against the remote server without requiring a local session.
    Used during first-time setup before the local DB exists.
    Returns the access/refresh tokens so the caller can store them after DB creation.
    """
    base_url = (getattr(request.app.state, "default_server_url", "") or _DEFAULT_SERVER_URL).rstrip("/")
    try:
        with httpx.Client(timeout=15) as client:
            res = client.post(
                f"{base_url}/auth/login",
                json={"email": req.email, "password": req.password},
            )
        if res.status_code == 401:
            raise HTTPException(status_code=401, detail="Invalid email or password")
        if not res.is_success:
            raise HTTPException(status_code=502, detail="Server error during login")
        data = res.json()
    except (httpx.ConnectError, httpx.ConnectTimeout, httpx.TimeoutException):
        raise HTTPException(status_code=503, detail="Cannot connect to server. Check your internet connection.")

    sub = data.get("subscription") or {}
    return {
        "ok":                      True,
        "email":                   req.email,
        "access_token":            data.get("access_token", ""),
        "refresh_token":           data.get("refresh_token", ""),
        "subscription_status":     sub.get("status", ""),
        "subscription_expires_at": sub.get("expires_at", ""),
    }


@router.get("/status")
def server_auth_status(conn: sqlite3.Connection = Depends(get_conn)):
    token = _get_setting(conn, "server_access_token") or ""
    return {
        "logged_in":                  bool(token),
        "email":                      _get_setting(conn, "server_user_email") or "",
        "subscription_status":        _get_setting(conn, "server_subscription_status") or "",
        "subscription_expires_at":    _get_setting(conn, "server_subscription_expires_at") or "",
    }


@router.post("/login")
def server_auth_login(req: LoginRequest, conn: sqlite3.Connection = Depends(get_conn)):
    base_url = _server_url(conn)
    try:
        with httpx.Client(timeout=15) as client:
            res = client.post(
                f"{base_url}/auth/login",
                json={"email": req.email, "password": req.password},
            )
        if res.status_code == 401:
            raise HTTPException(status_code=401, detail="Invalid email or password")
        if not res.is_success:
            raise HTTPException(status_code=502, detail="Server error during login")
        data = res.json()
    except (httpx.ConnectError, httpx.ConnectTimeout, httpx.TimeoutException):
        raise HTTPException(status_code=503, detail="Cannot connect to server. Check your internet connection.")

    _store_tokens(conn, data, email=req.email)
    sub = data.get("subscription") or {}
    return {
        "ok":                      True,
        "email":                   req.email,
        "subscription_status":     sub.get("status", ""),
        "subscription_expires_at": sub.get("expires_at", ""),
    }


@router.post("/logout")
def server_auth_logout(conn: sqlite3.Connection = Depends(get_conn)):
    refresh_token = _get_setting(conn, "server_refresh_token") or ""
    if refresh_token:
        base_url = _server_url(conn)
        try:
            with httpx.Client(timeout=10) as client:
                client.post(
                    f"{base_url}/auth/logout",
                    json={"refresh_token": refresh_token},
                )
        except Exception:
            pass  # Best-effort — clear tokens locally regardless
    _clear_tokens(conn)
    return {"ok": True}


@router.post("/refresh")
def server_auth_refresh(conn: sqlite3.Connection = Depends(get_conn)):
    refresh_token = _get_setting(conn, "server_refresh_token") or ""
    if not refresh_token:
        raise HTTPException(status_code=401, detail="No refresh token stored")
    base_url = _server_url(conn)
    try:
        with httpx.Client(timeout=15) as client:
            res = client.post(
                f"{base_url}/auth/refresh",
                json={"refresh_token": refresh_token},
            )
        if not res.is_success:
            _clear_tokens(conn)
            raise HTTPException(status_code=401, detail="Refresh token expired or invalid. Please login again.")
        data = res.json()
    except (httpx.ConnectError, httpx.ConnectTimeout, httpx.TimeoutException):
        raise HTTPException(status_code=503, detail="Cannot connect to server")

    _store_tokens(conn, data)
    return {"ok": True, "expires_in": data.get("expires_in")}


@router.post("/forgot-password")
def server_auth_forgot_password(req: ForgotPasswordRequest, conn: sqlite3.Connection = Depends(get_conn)):
    base_url = _server_url(conn)
    try:
        with httpx.Client(timeout=15) as client:
            client.post(f"{base_url}/auth/forgot-password", json={"email": req.email})
    except Exception:
        pass  # Always succeed — no enumeration
    return {"ok": True}


@router.get("/persons")
def server_auth_persons(conn: sqlite3.Connection = Depends(get_conn)):
    """
    Fetch the persons list from the server, decrypt it, cache locally, and return it.
    Falls back to the local cache if the server is unreachable.
    """
    token = _get_setting(conn, "server_access_token") or ""
    if not token:
        # Return cached persons even if not logged in (offline mode)
        return _cached_persons(conn)

    base_url = _server_url(conn)
    pub_key_b64 = ensure_keypair(conn)

    try:
        with httpx.Client(timeout=15) as client:
            res = client.get(
                f"{base_url}/persons/secure",
                headers={"Authorization": f"Bearer {token}", "X-Public-Key": pub_key_b64},
            )
        if res.status_code == 401:
            # Token expired — return cached data
            return _cached_persons(conn)
        if not res.is_success:
            return _cached_persons(conn)

        encrypted = res.json().get("data", "")
        persons_json = decrypt_server_response(conn, encrypted)
        persons = json.loads(persons_json)

        _upsert_persons_cache(conn, persons)
        return persons

    except (httpx.ConnectError, httpx.ConnectTimeout, httpx.TimeoutException):
        return _cached_persons(conn)
    except ValueError:
        return _cached_persons(conn)


def _cached_persons(conn: sqlite3.Connection) -> list:
    rows = conn.execute(
        """SELECT person_id, name, masked_pan, pan_hash,
                  subscription_status, subscription_expires_at, paid_price
           FROM persons ORDER BY person_id"""
    ).fetchall()
    return [dict(r) for r in rows]


def _upsert_persons_cache(conn: sqlite3.Connection, persons: list) -> None:
    now = datetime.now(timezone.utc).isoformat()
    for p in persons:
        conn.execute(
            """INSERT INTO persons
                   (person_id, name, display_name, masked_pan, pan_hash,
                    subscription_status, subscription_expires_at, paid_price)
               VALUES (:person_id, :display_name, :display_name, :masked_pan, :pan_hash,
                       :subscription_status, :expires_at, :paid_price)
               ON CONFLICT(person_id) DO UPDATE SET
                   name                    = excluded.name,
                   display_name            = excluded.display_name,
                   masked_pan              = excluded.masked_pan,
                   pan_hash                = excluded.pan_hash,
                   subscription_status     = excluded.subscription_status,
                   subscription_expires_at = excluded.subscription_expires_at,
                   paid_price              = excluded.paid_price""",
            {
                "person_id":           p.get("person_id"),
                "display_name":        p.get("display_name", ""),
                "masked_pan":          p.get("masked_pan"),
                "pan_hash":            p.get("pan_hash"),
                "subscription_status": p.get("subscription_status"),
                "expires_at":          p.get("expires_at"),
                "paid_price":          p.get("paid_price"),
            },
        )
    # Remove persons no longer on server
    if persons:
        server_ids = [p["person_id"] for p in persons if p.get("person_id")]
        placeholders = ",".join("?" * len(server_ids))
        conn.execute(
            f"DELETE FROM persons WHERE person_id NOT IN ({placeholders})", server_ids
        )
    _set_setting(conn, "persons_cached_at", now)
    conn.commit()
