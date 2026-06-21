"""Shared instrument resolution utilities — port of src-tauri/src/commands/import/common.rs."""
import json
import sqlite3


def resolve_equity(
    conn: sqlite3.Connection,
    name: str,
    isin: str | None = None,
    bse_code: str | None = None,
    nse_symbol: str | None = None,
    exchange: str | None = None,
) -> tuple[int | None, int | None]:
    """
    Try to resolve to (instrument_id, None) or (None, pending_id).
    Resolution order: ISIN → BSE code → NSE symbol → existing pending → new pending.
    """
    isin       = isin.strip()       if isin       else None
    bse_code   = bse_code.strip()   if bse_code   else None
    nse_symbol = nse_symbol.strip() if nse_symbol else None

    # 1. Resolve by ISIN
    if isin:
        row = conn.execute(
            "SELECT instrument_id FROM instrument_equity WHERE isin=? LIMIT 1", (isin,)
        ).fetchone()
        if row:
            return (row[0], None)

    # 2. Resolve by BSE code
    if bse_code:
        row = conn.execute(
            "SELECT instrument_id FROM instrument_equity WHERE bse_code=? LIMIT 1", (bse_code,)
        ).fetchone()
        if row:
            return (row[0], None)

    # 3. Resolve by NSE symbol
    if nse_symbol:
        row = conn.execute(
            "SELECT instrument_id FROM instrument_equity WHERE nse_symbol=? LIMIT 1", (nse_symbol,)
        ).fetchone()
        if row:
            return (row[0], None)

    # 4. Fuzzy name match against resolved instruments
    # Handles server names that are truncated vs full names in source files.
    # Strategy: find instruments whose name shares at least the first 20 characters
    # with the lookup name (case-insensitive). Guards against false positives by
    # requiring the shared prefix to be >= 20 chars.
    name_up = name.upper().strip()
    prefix20 = name_up[:20]
    if len(prefix20) >= 20:
        row = conn.execute(
            """SELECT ie.instrument_id FROM instrument_equity ie
               JOIN instruments i ON i.instrument_id = ie.instrument_id
               WHERE UPPER(SUBSTR(i.name, 1, 20)) = ?
               LIMIT 1""",
            (prefix20,),
        ).fetchone()
        if row:
            return (row[0], None)

    # 5. Look for existing pending instrument
    if isin:
        row = conn.execute(
            "SELECT pending_id FROM pending_instruments WHERE type='EQUITY' AND json_extract(metadata,'$.isin')=? LIMIT 1",
            (isin,),
        ).fetchone()
        if row:
            return (None, row[0])
    else:
        row = conn.execute(
            "SELECT pending_id FROM pending_instruments WHERE type='EQUITY' AND name=? LIMIT 1",
            (name,),
        ).fetchone()
        if row:
            return (None, row[0])

    # 6. Create new pending instrument
    metadata = {
        k: v for k, v in {
            "isin": isin, "bse_code": bse_code,
            "nse_symbol": nse_symbol, "exchange": exchange,
        }.items() if v
    }
    cur = conn.execute(
        "INSERT INTO pending_instruments (name, type, metadata) VALUES (?, 'EQUITY', ?)",
        (name, json.dumps(metadata)),
    )
    return (None, cur.lastrowid)


def resolve_mf(
    conn: sqlite3.Connection,
    name: str,
    isin: str | None = None,
    amfi_code: str | None = None,
) -> tuple[int | None, int | None]:
    """Resolve a mutual fund instrument."""
    isin      = isin.strip()      if isin      else None
    amfi_code = amfi_code.strip() if amfi_code else None

    if amfi_code:
        row = conn.execute(
            "SELECT instrument_id FROM instrument_mf WHERE amfi_code=? LIMIT 1", (amfi_code,)
        ).fetchone()
        if row:
            return (row[0], None)

    if isin:
        row = conn.execute(
            "SELECT im.instrument_id FROM instrument_mf im JOIN instrument_fixed_income fi ON fi.instrument_id=im.instrument_id WHERE fi.isin=? LIMIT 1",
            (isin,),
        ).fetchone()
        if row:
            return (row[0], None)

    # Check existing pending
    if amfi_code:
        row = conn.execute(
            "SELECT pending_id FROM pending_instruments WHERE type='MF' AND json_extract(metadata,'$.amfi_code')=? LIMIT 1",
            (amfi_code,),
        ).fetchone()
        if row:
            return (None, row[0])

    row = conn.execute(
        "SELECT pending_id FROM pending_instruments WHERE type='MF' AND name=? LIMIT 1",
        (name,),
    ).fetchone()
    if row:
        return (None, row[0])

    metadata = {k: v for k, v in {"isin": isin, "amfi_code": amfi_code}.items() if v}
    cur = conn.execute(
        "INSERT INTO pending_instruments (name, type, metadata) VALUES (?, 'MF', ?)",
        (name, json.dumps(metadata)),
    )
    return (None, cur.lastrowid)


def resolve_derivative(
    conn: sqlite3.Connection,
    name: str,
    underlying: str,
    expiry_date: str,
    pending_type: str,       # FUTSTK | FUTIDX | OPTSTK | OPTIDX
    instrument_type: str,    # FUTURES | OPTIONS
    exchange: str,
    strike_paise: int | None = None,
    option_type: str | None = None,
) -> tuple[int | None, int | None]:
    sym_up = underlying.upper()

    # 1. Look up instrument_derivatives
    if strike_paise is not None:
        row = conn.execute(
            """SELECT d.instrument_id FROM instrument_derivatives d
               WHERE UPPER(d.underlying_symbol)=? AND d.expiry_date=?
                 AND d.instrument_type=? AND d.strike_price_paise=? LIMIT 1""",
            (sym_up, expiry_date, instrument_type, strike_paise),
        ).fetchone()
    else:
        row = conn.execute(
            """SELECT d.instrument_id FROM instrument_derivatives d
               WHERE UPPER(d.underlying_symbol)=? AND d.expiry_date=?
                 AND d.instrument_type=? LIMIT 1""",
            (sym_up, expiry_date, instrument_type),
        ).fetchone()
    if row:
        return (row[0], None)

    # 2. Look up pending_instruments
    if strike_paise is not None:
        row = conn.execute(
            """SELECT pending_id FROM pending_instruments
               WHERE type=? AND UPPER(json_extract(metadata,'$.underlying_symbol'))=?
                 AND json_extract(metadata,'$.expiry_date')=?
                 AND CAST(json_extract(metadata,'$.strike_price_paise') AS INTEGER)=? LIMIT 1""",
            (pending_type, sym_up, expiry_date, strike_paise),
        ).fetchone()
    else:
        row = conn.execute(
            """SELECT pending_id FROM pending_instruments
               WHERE type=? AND UPPER(json_extract(metadata,'$.underlying_symbol'))=?
                 AND json_extract(metadata,'$.expiry_date')=? LIMIT 1""",
            (pending_type, sym_up, expiry_date),
        ).fetchone()
    if row:
        return (None, row[0])

    # 3. Create new pending
    metadata: dict = {"underlying_symbol": underlying, "expiry_date": expiry_date, "exchange": exchange}
    if strike_paise is not None:
        metadata["strike_price_paise"] = strike_paise
    if option_type:
        metadata["option_type"] = option_type

    cur = conn.execute(
        "INSERT INTO pending_instruments (name, type, metadata) VALUES (?, ?, ?)",
        (name, pending_type, json.dumps(metadata)),
    )
    return (None, cur.lastrowid)
