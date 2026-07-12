"""Price sync + instrument resolution — port of src-tauri/src/commands/prices.rs."""
import logging
import sqlite3
from datetime import datetime, timezone, timedelta

import httpx

_IST = timezone(timedelta(hours=5, minutes=30))
logger = logging.getLogger(__name__)


def _is_in_sync_window() -> bool:
    now = datetime.now(_IST).time()
    pre_market  = (now.hour,  now.minute) >= (6,  0)  and (now.hour, now.minute) <= (10, 30)
    post_close  = (now.hour,  now.minute) >= (15, 30) and (now.hour, now.minute) <= (23,  0)
    return pre_market or post_close


def _get_setting(conn: sqlite3.Connection, key: str) -> str | None:
    row = conn.execute("SELECT value FROM app_settings WHERE key = ?", (key,)).fetchone()
    return row[0] if row and row[0] else None


def _save_setting(conn: sqlite3.Connection, key: str, value: str) -> None:
    conn.execute(
        """INSERT INTO app_settings (key, value, updated_at)
           VALUES (?, ?, datetime('now'))
           ON CONFLICT(key) DO UPDATE SET value = excluded.value, updated_at = excluded.updated_at""",
        (key, value),
    )
    conn.commit()


def _server_url(conn: sqlite3.Connection) -> str:
    return (_get_setting(conn, "server_url") or "https://arthdeskapi.ashokitservices.com").rstrip("/")


def get_server_url(conn: sqlite3.Connection) -> str:
    return _server_url(conn)


def _get_auth_headers(conn: sqlite3.Connection) -> dict[str, str]:
    token = _get_setting(conn, "server_access_token") or ""
    if not token:
        return {}
    return {"Authorization": f"Bearer {token}"}


def _refresh_access_token(conn: sqlite3.Connection) -> dict[str, str]:
    """Try to refresh the server access token. Returns new headers or {}."""
    refresh_token = _get_setting(conn, "server_refresh_token") or ""
    if not refresh_token:
        return {}
    base_url = _server_url(conn)
    try:
        with httpx.Client(timeout=10) as client:
            res = client.post(f"{base_url}/auth/refresh", json={"refresh_token": refresh_token})
        if not res.is_success:
            # Refresh token expired — clear stored tokens
            _save_setting(conn, "server_access_token", "")
            _save_setting(conn, "server_refresh_token", "")
            return {}
        data = res.json()
        _save_setting(conn, "server_access_token",  data.get("access_token", ""))
        _save_setting(conn, "server_refresh_token", data.get("refresh_token", ""))
        return {"Authorization": f"Bearer {data['access_token']}"}
    except Exception:
        return {}


def _portfolio_instrument_ids(conn: sqlite3.Connection) -> list[int]:
    rows = conn.execute(
        "SELECT DISTINCT instrument_id FROM transactions WHERE instrument_id IS NOT NULL"
    ).fetchall()
    return [r[0] for r in rows]


def _get_pending_instruments(conn: sqlite3.Connection) -> list[dict]:
    rows = conn.execute(
        """SELECT pending_id,
                  type                                           AS instrument_type,
                  json_extract(metadata, '$.isin')              AS isin,
                  json_extract(metadata, '$.nse_symbol')        AS nse_symbol,
                  json_extract(metadata, '$.bse_code')          AS bse_code,
                  json_extract(metadata, '$.amfi_code')         AS amfi_code,
                  json_extract(metadata, '$.exchange')          AS exchange,
                  json_extract(metadata, '$.underlying_symbol') AS underlying_symbol,
                  json_extract(metadata, '$.expiry_date')       AS expiry_date,
                  json_extract(metadata, '$.strike_price_paise') AS strike_price_paise,
                  json_extract(metadata, '$.option_type')       AS contract_type,
                  json_extract(metadata, '$.mcx_symbol')        AS mcx_symbol,
                  json_extract(metadata, '$.unit')              AS unit
           FROM pending_instruments ORDER BY pending_id"""
    ).fetchall()
    return [dict(r) for r in rows]


def _sync_instrument_types(conn: sqlite3.Connection, client: httpx.Client, base_url: str) -> int:
    resp = client.get(f"{base_url}/instruments/types", timeout=30)
    resp.raise_for_status()
    data = resp.json()
    count = 0
    for t in data.get("instrument_types", []):
        conn.execute(
            """INSERT INTO instrument_types (instrument_type_id, name, asset_class, tax_category)
               VALUES (?, ?, ?, ?)
               ON CONFLICT(instrument_type_id) DO UPDATE SET
                   name = excluded.name,
                   asset_class = excluded.asset_class,
                   tax_category = excluded.tax_category""",
            (t["instrument_type_id"], t["name"], t["asset_class"], t["tax_category"]),
        )
        count += 1
    conn.commit()
    return count


def _apply_resolved_pending(conn: sqlite3.Connection, resolved: list[dict]) -> int:
    count = 0
    for r in resolved:
        exchange_id = None
        if r.get("primary_exchange_code"):
            row = conn.execute(
                "SELECT exchange_id FROM exchanges WHERE code = ?",
                (r["primary_exchange_code"],),
            ).fetchone()
            if row:
                exchange_id = row[0]

        conn.execute(
            """INSERT OR IGNORE INTO instruments
                   (instrument_id, name, instrument_type_id, primary_exchange_id, source)
               VALUES (?, ?, ?, ?, 'SERVER')""",
            (r["instrument_id"], r["name"], r["instrument_type_id"], exchange_id),
        )

        itype = r.get("instrument_type_name", "")
        if itype == "EQUITY":
            conn.execute(
                """INSERT OR IGNORE INTO instrument_equity
                       (instrument_id, isin, nse_symbol, nse_fininstrmid, bse_code)
                   VALUES (?, ?, ?, ?, ?)""",
                (r["instrument_id"], r.get("isin"), r.get("nse_symbol"),
                 r.get("nse_equity_fininstrmid"), r.get("bse_code")),
            )
        elif itype == "INDEX":
            conn.execute(
                "INSERT OR IGNORE INTO instrument_index (instrument_id, symbol, exchange) VALUES (?, ?, ?)",
                (r["instrument_id"], r.get("index_symbol"), r.get("index_exchange")),
            )
        elif itype in ("EQUITY_MF", "DEBT_MF", "HYBRID_MF", "ELSS", "SIF"):
            conn.execute(
                "INSERT OR IGNORE INTO instrument_mf (instrument_id, amfi_code) VALUES (?, ?)",
                (r["instrument_id"], r.get("amfi_code")),
            )
        elif itype in ("FUTURES", "OPTIONS"):
            conn.execute(
                """INSERT OR IGNORE INTO instrument_derivatives
                       (instrument_id, underlying_instrument_id, underlying_symbol,
                        expiry_date, lot_size, strike_price_paise,
                        instrument_type, option_type, nse_fininstrmid, bse_fininstrmid)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?, ?, ?)""",
                (r["instrument_id"], r.get("underlying_instrument_id"),
                 r.get("underlying_symbol"), r.get("fo_expiry_date"),
                 r.get("fo_lot_size") or 0, r.get("fo_strike_price_paise"),
                 r.get("fo_instrument_type"), r.get("fo_option_type"),
                 r.get("fo_nse_fininstrmid"), r.get("fo_bse_fininstrmid")),
            )
        elif itype in ("COMMODITY_FUTURES", "COMMODITY_OPTIONS"):
            conn.execute(
                """INSERT OR IGNORE INTO instrument_mcx
                       (instrument_id, mcx_symbol, instrument_type, expiry_date,
                        lot_size, unit, strike_price_paise, option_type)
                   VALUES (?, ?, ?, ?, ?, ?, ?, ?)""",
                (r["instrument_id"], r.get("mcx_symbol"), r.get("mcx_instrument_type"),
                 r.get("mcx_expiry_date"), r.get("mcx_lot_size") or 0.0,
                 r.get("mcx_unit"), r.get("mcx_strike_price_paise"), r.get("mcx_option_type")),
            )

        conn.execute(
            """UPDATE transactions
               SET instrument_id = ?, pending_instrument_id = NULL
               WHERE pending_instrument_id = ?""",
            (r["instrument_id"], r["pending_id"]),
        )
        conn.execute(
            "DELETE FROM pending_instruments WHERE pending_id = ?", (r["pending_id"],)
        )
        count += 1

    conn.commit()
    return count


def resolve_instruments(conn: sqlite3.Connection) -> dict:
    pending = _get_pending_instruments(conn)
    if not pending:
        return {"resolved": 0, "unresolved": 0}

    total = len(pending)
    base_url = _server_url(conn)
    logger.info("Resolving %d pending instrument(s) via %s", total, base_url)

    headers = _get_auth_headers(conn)
    with httpx.Client(timeout=30) as client:
        logger.info("Fetching instrument types from %s/instruments/types", base_url)
        _sync_instrument_types(conn, client, base_url)

        logger.info("Resolving pending instruments at %s/instruments/resolve", base_url)
        resp = client.post(f"{base_url}/instruments/resolve", json=pending, headers=headers)
        if resp.status_code == 401:
            headers = _refresh_access_token(conn)
            if not headers:
                return {"resolved": 0, "unresolved": total, "message": "Server login required"}
            resp = client.post(f"{base_url}/instruments/resolve", json=pending, headers=headers)
        if resp.status_code in (401, 403):
            return {"resolved": 0, "unresolved": total, "message": "Server login required or subscription inactive"}
        resp.raise_for_status()
        body = resp.json()
        resolved_list = body.get("resolved", [])

        # Re-sync types if any returned type_id is unknown locally
        for r in resolved_list:
            row = conn.execute(
                "SELECT 1 FROM instrument_types WHERE instrument_type_id = ?",
                (r.get("instrument_type_id"),),
            ).fetchone()
            if row is None:
                _sync_instrument_types(conn, client, base_url)
                break

        resolved = _apply_resolved_pending(conn, resolved_list)

    return {"resolved": resolved, "unresolved": max(0, total - resolved)}


def sync_prices(conn: sqlite3.Connection, force: bool = False) -> dict:
    if not force and not _is_in_sync_window():
        return {"updated": 0, "synced_at": "", "message": "Outside automatic sync window"}

    ids = _portfolio_instrument_ids(conn)
    if not ids:
        pending_count = len(_get_pending_instruments(conn))
        if pending_count:
            return {
                "updated": 0,
                "synced_at": "",
                "message": f"{pending_count} pending instrument(s) could not be resolved yet",
            }
        return {"updated": 0, "synced_at": "", "message": "No resolved instruments in portfolio"}

    base_url  = _server_url(conn)
    last_sync = _get_setting(conn, "last_price_sync") or ""
    logger.info("Syncing prices for %d instrument(s) via %s", len(ids), base_url)

    id_qs = "&".join(f"instrument_ids={i}" for i in ids)
    if force or not last_sync:
        url = f"{base_url}/prices/sync?{id_qs}"
    else:
        url = f"{base_url}/prices/sync?{id_qs}&since_datetime={last_sync}"

    headers = _get_auth_headers(conn)
    with httpx.Client(timeout=30) as client:
        logger.info("Fetching prices from %s", url)
        resp = client.get(url, headers=headers)
        if resp.status_code == 401:
            headers = _refresh_access_token(conn)
            if not headers:
                return {"updated": 0, "synced_at": "", "message": "Server login required"}
            resp = client.get(url, headers=headers)
        if resp.status_code in (401, 403):
            return {"updated": 0, "synced_at": "", "message": "Server login required or subscription inactive"}
        resp.raise_for_status()
        body = resp.json()

        updated = 0
        for p in body.get("prices", []):
            conn.execute(
                """INSERT INTO latest_prices
                       (instrument_id, price_date,
                        open_price_paise, high_price_paise, low_price_paise,
                        close_price_paise, updated_at)
                   VALUES (?, ?, ?, ?, ?, ?, datetime('now'))
                   ON CONFLICT(instrument_id) DO UPDATE SET
                       price_date        = excluded.price_date,
                       open_price_paise  = COALESCE(excluded.open_price_paise, open_price_paise),
                       high_price_paise  = COALESCE(excluded.high_price_paise, high_price_paise),
                       low_price_paise   = COALESCE(excluded.low_price_paise,  low_price_paise),
                       close_price_paise = excluded.close_price_paise,
                       updated_at        = datetime('now')""",
                (p["instrument_id"], p["price_date"],
                 p.get("open_price_paise"), p.get("high_price_paise"),
                 p.get("low_price_paise"), p["close_price_paise"]),
            )
            updated += 1
        conn.commit()

        synced_at = body.get("synced_at") or datetime.now(timezone.utc).strftime("%Y-%m-%dT%H:%M:%S")
        _save_setting(conn, "last_price_sync", synced_at)

        # Best-effort instrument metadata delta sync
        try:
            _fetch_instrument_updates(conn, client, base_url, headers=headers)
        except Exception:
            pass

    return {"updated": updated, "synced_at": synced_at}


def _fetch_instrument_updates(conn: sqlite3.Connection, client: httpx.Client, base_url: str, headers: dict | None = None) -> None:
    ids = _portfolio_instrument_ids(conn)
    if not ids:
        return
    last_sync = _get_setting(conn, "last_instrument_sync") or ""
    id_qs = "&".join(f"instrument_ids={i}" for i in ids)
    url = f"{base_url}/instruments/updates?{id_qs}"
    if last_sync:
        url += f"&since={last_sync}"

    resp = client.get(url, timeout=30, headers=headers or {})
    if resp.status_code in (401, 403):
        return  # Silently skip instrument updates if auth fails
    resp.raise_for_status()
    body = resp.json()

    for u in body.get("updates", []):
        conn.execute(
            "UPDATE instruments SET name = ?, instrument_type_id = ?, updated_at = ? WHERE instrument_id = ?",
            (u["name"], u["instrument_type_id"], u.get("updated_at", ""), u["instrument_id"]),
        )
        itype = u.get("instrument_type_name", "")
        if itype == "EQUITY":
            conn.execute(
                """UPDATE instrument_equity SET
                       isin       = COALESCE(?, isin),
                       nse_symbol = COALESCE(?, nse_symbol),
                       bse_code   = COALESCE(?, bse_code),
                       sector     = COALESCE(?, sector),
                       industry   = COALESCE(?, industry)
                   WHERE instrument_id = ?""",
                (u.get("isin"), u.get("nse_symbol"), u.get("bse_code"),
                 u.get("sector"), u.get("industry"), u["instrument_id"]),
            )
        elif itype in ("EQUITY_MF", "DEBT_MF", "HYBRID_MF", "ELSS", "SIF"):
            conn.execute(
                "UPDATE instrument_mf SET amfi_code = COALESCE(?, amfi_code) WHERE instrument_id = ?",
                (u.get("amfi_code"), u["instrument_id"]),
            )
    conn.commit()
    _save_setting(conn, "last_instrument_sync", body.get("synced_at", ""))
