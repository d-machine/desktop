"""
Bajaj Financial Securities contract note parser.

PDF is always password-protected. Password is usually the client's PAN.
Falls back to prompting the user if PAN doesn't work.

Page 1  — trade date, contract note number, summary table (one row per ISIN)
Page 2  — charges: STT, GST, exchange charges, stamp duty, SEBI fees
Pages 2+ — individual trade executions (annexure — we ignore, use summary instead)
"""
import re
import sqlite3
from datetime import datetime

from importers.pdf_utils import open_pdf
from importers.common import resolve_equity

# ── Regex patterns ────────────────────────────────────────────────────────────

_DATE_RE      = re.compile(r'Trade\s+Date\s+(\d{2}-[A-Za-z]{3}-\d{4})')
_CONTRACT_RE  = re.compile(r'Contract\s+Note\s+No[./]Invoice\s+No\s+(\S+)')
_CLIENT_RE    = re.compile(r'Client\s+Code\s*[:\s]+(\S+)')

# Summary table row: ISIN [optional security name] BuyQty BuyAvg BuyExch BuyNet BuyObl SellQty SellAvg SellExch SellNet SellObl NetQty NetObl
_SUMMARY_ROW  = re.compile(
    r'^(IN[A-Z0-9]{10})'            # ISIN
    r'(?:\s+([A-Z][A-Z0-9 &@().\'/-]*?))?' # Optional security name
    r'\s+(\d+)'                     # Buy Qty
    r'\s+([\d.]+)'                  # Buy Avg Price
    r'\s+([\d.]+)'                  # Buy Exchange charges per unit
    r'\s+([\d.]+)'                  # Buy Net Price
    r'\s+([-\d.]+)'                 # Buy Obligation
    r'\s+(\d+)'                     # Sell Qty
    r'\s+([\d.]+)'                  # Sell Avg Price
    r'\s+([\d.]+)'                  # Sell Exchange charges per unit
    r'\s+([\d.]+)'                  # Sell Net Price
    r'\s+([-\d.]+)'                 # Sell Obligation
    r'\s+(-?\d+)'                   # Net Qty
    r'\s+([-\d.]+)',                # Net Obligation
    re.MULTILINE,
)

# Charges
_STT_RE       = re.compile(r'Securities\s+Transaction\s+Tax.*?([\d,]+\.\d+)')
_CGST_RE      = re.compile(r'CGST.*?Amount.*?([\d,]+\.\d+)', re.DOTALL)
_SGST_RE      = re.compile(r'SGST.*?Amount.*?([\d,]+\.\d+)', re.DOTALL)
_IGST_RE      = re.compile(r'IGST.*?Amount.*?([\d,]+\.\d+)', re.DOTALL)
_EXCH_RE      = re.compile(r'Exchange\s+Transaction\s+Charges.*?([\d,]+\.\d+)')
_SEBI_RE      = re.compile(r'SEBI\s+turnover\s+Fees.*?([\d,]+\.\d+)')
_STAMP_RE     = re.compile(r'Stamp\s+Duty.*?([\d,]+\.\d+)')
_IPFT_RE      = re.compile(r'IPFT\s+Charges\s+([\d,]+\.\d+)')
_NET_AMT_RE   = re.compile(r'Net\s+Amount\s+Receivable.*?Payable.*?\(.*?\)\)\s*([-\d,.]+)')

_MONTH = {
    'jan': '01', 'feb': '02', 'mar': '03', 'apr': '04',
    'may': '05', 'jun': '06', 'jul': '07', 'aug': '08',
    'sep': '09', 'oct': '10', 'nov': '11', 'dec': '12',
}


def _parse_date(raw: str) -> str:
    """Convert '07-Apr-2026' → '2026-04-07'."""
    parts = raw.strip().split('-')
    if len(parts) != 3:
        return raw
    day, mon, year = parts
    return f"{year}-{_MONTH.get(mon.lower(), '01')}-{day.zfill(2)}"


def _parse_amount(s: str) -> int:
    """Parse a rupee amount string to paise."""
    return round(float(s.replace(',', '')) * 100)


def parse(file_path: str, password: str | None = None) -> dict:
    """
    Parse a Bajaj Finance contract note PDF.
    Returns a dict with keys: trades, charges, trade_date, contract_note_no, client_code.
    """
    with open_pdf(file_path, password) as pdf:
        pages = [p.extract_text() or "" for p in pdf.pages]

    if not pages:
        raise ValueError("Could not extract text from PDF")

    page1 = pages[0]
    # Charges are on page 2
    page2 = pages[1] if len(pages) > 1 else ""

    # ── Header fields ─────────────────────────────────────────────────────────
    date_m = _DATE_RE.search(page1)
    if not date_m:
        raise ValueError("Could not find Trade Date in PDF")
    trade_date = _parse_date(date_m.group(1))

    contract_m = _CONTRACT_RE.search(page1)
    contract_note_no = contract_m.group(1) if contract_m else None

    client_m = _CLIENT_RE.search(page1)
    client_code = client_m.group(1) if client_m else None

    # ── Summary table ─────────────────────────────────────────────────────────
    # Lines can span multiple rows due to long security names.
    # Strategy: scan all lines, track the line before each ISIN line as a
    # potential security name prefix.
    trades = []
    lines = page1.splitlines()

    for i, line in enumerate(lines):
        m = _SUMMARY_ROW.match(line.strip())
        if not m:
            continue

        isin          = m.group(1)
        name_on_line  = (m.group(2) or "").strip()
        buy_qty       = int(m.group(3))
        buy_gross_avg  = float(m.group(4))   # gross WAP
        buy_exch       = float(m.group(5))   # exchange charges per unit
        buy_avg        = float(m.group(6))   # net price (WAP minus exchange charges)
        sell_qty       = int(m.group(8))
        sell_gross_avg = float(m.group(9))   # gross WAP
        sell_exch      = float(m.group(10))  # exchange charges per unit
        sell_avg       = float(m.group(11))  # net price (WAP minus exchange charges)

        # If name not on this line, check the line above (and possibly below)
        if not name_on_line:
            prev = lines[i - 1].strip() if i > 0 else ""
            nxt  = lines[i + 1].strip() if i + 1 < len(lines) else ""
            # Accept previous line as name if it looks like a security name
            # (no ISIN, not a number, not a known header keyword)
            if prev and not _SUMMARY_ROW.match(prev) and not prev[0].isdigit() and 'ISIN' not in prev:
                name_on_line = prev
                # Check if next line continues the name (e.g. "VENTURES LTD")
                if nxt and not _SUMMARY_ROW.match(nxt) and not nxt[0].isdigit() and 'Total' not in nxt:
                    name_on_line = f"{name_on_line} {nxt}"
            name_on_line = name_on_line.strip()

        security_name = name_on_line or isin

        if buy_qty > 0:
            trades.append({
                "isin":           isin,
                "name":           security_name,
                "side":           "BUY",
                "quantity":       buy_qty,
                "price_rs":       buy_avg,
                "gross_price_rs": buy_gross_avg,
                "exch_charges_rs": buy_exch,
                "trade_date":     trade_date,
            })

        if sell_qty > 0:
            trades.append({
                "isin":            isin,
                "name":            security_name,
                "side":            "SELL",
                "quantity":        sell_qty,
                "price_rs":        sell_avg,
                "gross_price_rs":  sell_gross_avg,
                "exch_charges_rs": sell_exch,
                "trade_date":      trade_date,
            })

    if not trades:
        raise ValueError("No trades found in summary table — check PDF format or password")

    # ── Charges ───────────────────────────────────────────────────────────────
    charges_text = page2

    def _extract(pattern: re.Pattern, text: str) -> int:
        m = pattern.search(text)
        return _parse_amount(m.group(1)) if m else 0

    stt_paise       = _extract(_STT_RE,   charges_text)
    cgst_paise      = _extract(_CGST_RE,  charges_text)
    sgst_paise      = _extract(_SGST_RE,  charges_text)
    igst_paise      = _extract(_IGST_RE,  charges_text)
    exch_paise      = _extract(_EXCH_RE,  charges_text)
    sebi_paise      = _extract(_SEBI_RE,  charges_text)
    stamp_paise     = _extract(_STAMP_RE, charges_text)
    ipft_paise      = _extract(_IPFT_RE,  charges_text)
    gst_paise       = cgst_paise + sgst_paise + igst_paise
    other_paise     = sebi_paise + ipft_paise

    net_m = _NET_AMT_RE.search(charges_text)
    total_payable_paise = _parse_amount(net_m.group(1)) if net_m else 0

    return {
        "trades":             trades,
        "trade_date":         trade_date,
        "contract_note_no":   contract_note_no,
        "client_code":        client_code,
        "stt_paise":          stt_paise,
        "stamp_paise":        stamp_paise,
        "gst_paise":          gst_paise,
        "exchange_paise":     exch_paise,
        "other_paise":        other_paise,
        "total_payable_paise":total_payable_paise,
    }


def import_(
    conn: sqlite3.Connection,
    account_id: int,
    data: dict,
    batch_id: int | None = None,
) -> dict:
    imported = 0
    skipped  = 0
    auto_created: list[str] = []

    for trade in data["trades"]:
        # Resolve instrument by ISIN
        instrument_id, pending_id = resolve_equity(
            conn,
            name=trade["name"],
            isin=trade["isin"],
        )

        if instrument_id is None and pending_id is None:
            skipped += 1
            continue

        if trade["name"] not in auto_created and pending_id is not None:
            # Only note it if it's truly new (resolve_equity may have found existing)
            pass  # resolve_equity handles dedup of pending instruments

        # Dedup by broker_ref (contract note no + ISIN + side + trade date)
        broker_ref = f"{trade.get('contract_note_no', data.get('contract_note_no',''))}-{trade['isin']}-{trade['side']}-{trade['trade_date']}"
        if broker_ref:
            exists = conn.execute(
                "SELECT 1 FROM transactions WHERE account_id=? AND broker_ref=? LIMIT 1",
                (account_id, broker_ref),
            ).fetchone()
            if exists:
                skipped += 1
                continue

        # Bajaj: actual=gross WAP, brokerage=exchange charges/unit, effective=net price
        # Store as REAL paise to preserve 4-decimal-place precision from contract note
        actual_paise    = trade.get("gross_price_rs", trade["price_rs"]) * 100
        brokerage_paise = trade.get("exch_charges_rs", 0) * 100
        effective_paise = trade["price_rs"] * 100  # net price already

        conn.execute(
            """INSERT INTO transactions
                   (account_id, instrument_id, pending_instrument_id,
                    txn_type, trade_segment, trade_date,
                    quantity, actual_price_paise, brokerage_per_unit_paise,
                    effective_price_paise, stt_paise, other_charges_paise,
                    broker_ref, batch_id)
               VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?,?)""",
            (account_id, instrument_id, pending_id,
             trade["side"], "DELIVERY", trade["trade_date"],
             trade["quantity"], actual_paise, brokerage_paise,
             effective_paise, 0, 0,
             broker_ref, batch_id),
        )
        imported += 1

    conn.commit()
    return {"imported": imported, "skipped": skipped, "auto_created_instruments": auto_created}
