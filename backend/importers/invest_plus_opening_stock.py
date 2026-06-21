"""
Invest Plus — Trading Opening Stock Report parser (.xls)

The file contains FIFO lots held as of 31 March (financial year end).
Structure:
  Row 1:  Report title
  Row 3:  Portfolio name
  Row 4:  Financial Year (e.g. "2026 - 2027" → opening date 31-03-2026)
  Row 6:  Column headers: Purchase Date | Qty | Rate | Amount | Narration
  Then:
    [Broker name row]       — text row, no numbers
    [Stock name row]        — text row, no numbers
    [Lot rows]              — [excel_date, qty, rate, amount, narration]
    "Total : "              — per-stock total (skip)
    "Broker Total : "       — per-broker total (skip)
  "Grand Total : "          — end of file

Each lot is imported as an OPENING_BALANCE transaction using the original
purchase date so that FIFO and holding-period calculations are correct.
"""
import re
import sqlite3
from datetime import date

import xlrd

from importers.common import resolve_equity

_SKIP_PREFIXES = ("Total :", "Broker Total :", "Grand Total :", "Purchase Date")
_BROKER_KEYWORDS = (
    "SECURITIES", "BROKING", "EQUITY", "FINANCIAL", "CAPITAL",
    "SHARES", "STOCK", "INVEST", "WEALTH",
)


def _is_broker_name(name: str) -> bool:
    up = name.upper()
    return any(kw in up for kw in _BROKER_KEYWORDS)


def _excel_date_to_iso(serial: float, datemode: int = 0) -> str:
    try:
        dt = xlrd.xldate_as_datetime(int(serial), datemode)
        return dt.strftime("%Y-%m-%d")
    except Exception:
        return ""


def _parse_fy(fy_str: str) -> str:
    """
    "2026 - 2027" → "2026-03-31"  (opening stock = end of FY-1 = 31 Mar 2026)
    """
    m = re.search(r"(\d{4})\s*[-–]\s*\d{4}", fy_str)
    if m:
        return f"{m.group(1)}-03-31"
    return "2026-03-31"


def parse(file_path: str, password: str | None = None) -> dict:
    """
    Parse an Invest Plus Opening Stock .xls file.
    Returns dict with keys: lots, portfolio_name, financial_year, opening_date.
    """
    wb = xlrd.open_workbook(file_path)
    ws = wb.sheets()[0]
    datemode = wb.datemode

    portfolio_name = ""
    financial_year = ""
    opening_date   = ""

    lots: list[dict] = []
    current_stock  = ""
    current_broker = ""

    for i in range(ws.nrows):
        row = ws.row_values(i)
        col0 = str(row[0]).strip() if row[0] not in ("", None) else ""

        # ── Metadata rows ─────────────────────────────────────────────────────
        if col0.startswith("Portfolio :"):
            portfolio_name = col0.replace("Portfolio :", "").strip()
            continue
        if col0.startswith("Financial Year :"):
            financial_year = col0.replace("Financial Year :", "").strip()
            opening_date   = _parse_fy(financial_year)
            continue

        # ── Skip headers and total rows ───────────────────────────────────────
        if any(col0.startswith(p) for p in _SKIP_PREFIXES):
            continue

        # ── Lot row: col0 is an Excel date serial (float > 30000) ────────────
        if isinstance(row[0], float) and row[0] > 30000:
            qty    = float(row[1]) if row[1] not in ("", None) else 0.0
            rate   = float(row[2]) if row[2] not in ("", None) else 0.0
            amount = float(row[3]) if row[3] not in ("", None) else 0.0

            if qty <= 0 or not current_stock:
                continue

            trade_date = _excel_date_to_iso(row[0], datemode)
            if not trade_date:
                trade_date = opening_date

            lots.append({
                "broker":      current_broker,
                "name":        current_stock,
                "quantity":    qty,
                "price_rs":    rate,
                "amount_rs":   amount,
                "trade_date":  trade_date,
            })
            continue

        # ── Name row: col0 is text, other cols are empty ─────────────────────
        if col0 and all(v in ("", None, 0.0) for v in row[1:]):
            if _is_broker_name(col0):
                current_broker = col0
                current_stock  = ""
            else:
                current_stock = col0
            continue

    return {
        "lots":           lots,
        "portfolio_name": portfolio_name,
        "financial_year": financial_year,
        "opening_date":   opening_date,
        "total_lots":     len(lots),
    }


def import_(
    conn: sqlite3.Connection,
    account_id: int,
    data: dict,
    batch_id: int | None = None,
) -> dict:
    """
    Import each lot as an OPENING_BALANCE transaction.
    Dedup key: account_id + broker_ref (OB-{name}-{trade_date}).
    """
    imported = 0
    skipped  = 0

    for lot in data["lots"]:
        # broker_ref uniquely identifies this lot for dedup
        safe_name  = re.sub(r"[^A-Z0-9]", "_", lot["name"].upper())[:25]
        broker_ref = f"OB-{safe_name}-{lot['trade_date'].replace('-', '')}"

        # Dedup
        exists = conn.execute(
            "SELECT 1 FROM transactions WHERE account_id=? AND broker_ref=? LIMIT 1",
            (account_id, broker_ref),
        ).fetchone()
        if exists:
            skipped += 1
            continue

        # Resolve instrument by name (no ISIN available)
        instrument_id, pending_id = resolve_equity(
            conn,
            name=lot["name"],
        )

        qty = lot["quantity"]
        # Invest Plus gives only a lump amount — no breakdown between actual and brokerage.
        # Use amount/qty as effective rate; leave actual and brokerage NULL.
        if lot["amount_rs"] and qty > 0:
            effective_paise = lot["amount_rs"] * 100 / qty
        else:
            effective_paise = lot["price_rs"] * 100

        conn.execute(
            """INSERT INTO transactions
                   (account_id, instrument_id, pending_instrument_id,
                    txn_type, trade_segment, trade_date,
                    quantity, effective_price_paise,
                    stt_paise, other_charges_paise, broker_ref, batch_id)
               VALUES (?,?,?,'OPENING_BALANCE','DELIVERY',?,?,?,0,0,?,?)""",
            (account_id, instrument_id, pending_id,
             lot["trade_date"], qty, effective_paise,
             broker_ref, batch_id),
        )
        imported += 1

    conn.commit()
    return {"imported": imported, "skipped": skipped}
