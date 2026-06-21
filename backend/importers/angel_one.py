"""Angel One Trades & Charges .xlsx parser — port of src-tauri/src/commands/import/angel_one.rs."""
import sqlite3
from datetime import datetime

import openpyxl


def _cell_str(cell) -> str:
    if cell.value is None:
        return ""
    return str(cell.value).strip()


def _cell_float(cell) -> float:
    v = cell.value
    if v is None:
        return 0.0
    if isinstance(v, (int, float)):
        return float(v)
    try:
        return float(str(v).strip().replace(",", ""))
    except ValueError:
        return 0.0


def _cell_date(cell) -> str:
    v = cell.value
    if v is None:
        return ""
    if isinstance(v, datetime):
        return v.strftime("%Y-%m-%d")
    s = str(v).strip()
    return s[:10] if len(s) >= 10 else s


def _map_segment(order_type: str, segment: str) -> str:
    if segment.upper() == "FUTURES":
        return "DERIVATIVES"
    if order_type.upper() == "INTRADAY":
        return "INTRADAY"
    return "DELIVERY"


def parse(file_path: str, password: str | None = None) -> dict:
    wb = openpyxl.load_workbook(file_path, data_only=True)
    ws = wb.active

    rows = list(ws.iter_rows())

    # Extract metadata from header rows (first 10 rows)
    client_code = ""
    start_date  = ""
    end_date    = ""
    for row in rows[:10]:
        label = _cell_str(row[0])
        if label == "ClientCode":
            client_code = _cell_str(row[1]) if len(row) > 1 else ""
        elif label == "StartDate":
            raw = _cell_str(row[1]) if len(row) > 1 else ""
            start_date = raw[:10]
        elif label == "EndDate":
            raw = _cell_str(row[1]) if len(row) > 1 else ""
            end_date = raw[:10]

    # Find header row (first cell = "Scrip/Contract")
    header_idx = None
    for i, row in enumerate(rows):
        if row and _cell_str(row[0]) == "Scrip/Contract":
            header_idx = i
            break
    if header_idx is None:
        raise ValueError("Could not find trade data header row in Angel One file")

    # Collect raw rows
    # Columns: 0:Scrip 1:Buy/Sell 2:BuyPrice 3:SellPrice 4:Qty
    #  5:Brokerage 6:GST 7:STT 8:SebiTax 9:ExchangeCharges
    #  10:StampDuty 11:OtherCharges 12:IPFTCharges
    #  13:OrderType 14:Segment 15:Exchange 16:OrderID 17:TradeID 18:Date

    def _get(row, idx):
        return row[idx] if idx < len(row) else None

    raw_rows = []
    for row in rows[header_idx + 1:]:
        if all(c.value is None for c in row):
            continue
        scrip = _cell_str(_get(row, 0))
        if not scrip:
            continue
        raw_rows.append({
            "scrip":            scrip,
            "side":             _cell_str(_get(row, 1)),
            "buy_price":        _cell_float(_get(row, 2)),
            "sell_price":       _cell_float(_get(row, 3)),
            "qty":              _cell_float(_get(row, 4)),
            "brokerage":        _cell_float(_get(row, 5)),
            "gst":              _cell_float(_get(row, 6)),
            "stt":              _cell_float(_get(row, 7)),
            "sebi_tax":         _cell_float(_get(row, 8)),
            "exchange_charges": _cell_float(_get(row, 9)),
            "stamp_duty":       _cell_float(_get(row, 10)),
            "other_charges":    _cell_float(_get(row, 11)) + _cell_float(_get(row, 12)),
            "order_type":       _cell_str(_get(row, 13)),
            "segment":          _cell_str(_get(row, 14)),
            "exchange":         _cell_str(_get(row, 15)),
            "order_id":         _cell_str(_get(row, 16)),
            "trade_id":         _cell_str(_get(row, 17)),
            "trade_date":       _cell_date(_get(row, 18)),
        })

    # Build brokerage/GST map from charge rows (trade_id empty)
    charge_map: dict[str, tuple[float, float]] = {}
    for r in raw_rows:
        if not r["trade_id"]:
            key = r["order_id"]
            existing = charge_map.get(key, (0.0, 0.0))
            charge_map[key] = (existing[0] + r["brokerage"], existing[1] + r["gst"])

    # Process trade rows
    trades = []
    unmatched_scrips = set()
    for r in raw_rows:
        if not r["trade_id"]:
            continue
        side_upper = r["side"].upper()
        price = r["buy_price"] if side_upper == "BUY" else r["sell_price"]
        brokerage, gst = charge_map.get(r["order_id"], (0.0, 0.0))
        total_charges = (brokerage + gst + r["stt"] + r["exchange_charges"]
                         + r["stamp_duty"] + r["sebi_tax"] + r["other_charges"])
        segment = _map_segment(r["order_type"], r["segment"])

        unmatched_scrips.add(r["scrip"])
        trades.append({
            "trade_date":          r["trade_date"],
            "scrip_name":          r["scrip"],
            "side":                side_upper,
            "price_rs":            price,
            "quantity":            r["qty"],
            "segment":             segment,
            "exchange":            r["exchange"],
            "order_id":            r["order_id"],
            "trade_id":            r["trade_id"],
            "brokerage_rs":        brokerage,
            "gst_rs":              gst,
            "stt_rs":              r["stt"],
            "exchange_charges_rs": r["exchange_charges"],
            "stamp_duty_rs":       r["stamp_duty"],
            "sebi_tax_rs":         r["sebi_tax"],
            "other_charges_rs":    r["other_charges"],
            "total_charges_rs":    total_charges,
        })

    trades.sort(key=lambda t: t["trade_date"])

    return {
        "trades":           trades,
        "unmatched_scrips": list(unmatched_scrips),
        "date_range":       [start_date, end_date],
        "client_code":      client_code,
    }


def import_(
    conn: sqlite3.Connection,
    account_id: int,
    data: dict,
    batch_id: int | None = None,
) -> dict:
    trades = data["trades"]

    # Get equity type_id and NSE exchange_id
    row = conn.execute(
        "SELECT instrument_type_id FROM instrument_types WHERE name='EQUITY' LIMIT 1"
    ).fetchone()
    equity_type_id = row[0] if row else 1

    row = conn.execute("SELECT exchange_id FROM exchanges WHERE code='NSE' LIMIT 1").fetchone()
    nse_exchange_id = row[0] if row else None

    imported = 0
    skipped  = 0
    auto_created: list[str] = []

    for trade in trades:
        scrip_upper = trade["scrip_name"].upper()

        # 1. NSE symbol match
        row = conn.execute(
            "SELECT ie.instrument_id FROM instrument_equity ie WHERE UPPER(ie.nse_symbol)=? LIMIT 1",
            (scrip_upper,),
        ).fetchone()
        instrument_id = row[0] if row else None

        # 2. Name match
        if instrument_id is None:
            row = conn.execute(
                "SELECT instrument_id FROM instruments WHERE UPPER(name)=? LIMIT 1",
                (scrip_upper,),
            ).fetchone()
            instrument_id = row[0] if row else None

        # 3. Auto-create placeholder
        if instrument_id is None:
            conn.execute(
                "INSERT OR IGNORE INTO instruments (name, instrument_type_id, primary_exchange_id, source) VALUES (?,?,?,'IMPORT')",
                (trade["scrip_name"], equity_type_id, nse_exchange_id),
            )
            row = conn.execute(
                "SELECT instrument_id FROM instruments WHERE name=? ORDER BY instrument_id DESC LIMIT 1",
                (trade["scrip_name"],),
            ).fetchone()
            instrument_id = row[0]
            conn.execute(
                "INSERT OR IGNORE INTO instrument_equity (instrument_id, nse_symbol) VALUES (?,?)",
                (instrument_id, scrip_upper),
            )
            if trade["scrip_name"] not in auto_created:
                auto_created.append(trade["scrip_name"])

        # Deduplicate by (account_id, broker_ref)
        if trade["trade_id"]:
            exists = conn.execute(
                "SELECT 1 FROM transactions WHERE account_id=? AND broker_ref=? LIMIT 1",
                (account_id, trade["trade_id"]),
            ).fetchone()
            if exists:
                skipped += 1
                continue

        actual_paise    = trade["price_rs"] * 100
        brokerage_paise = trade["brokerage_rs"] * 100
        stt_paise       = round(trade["stt_rs"] * 100)
        other_paise     = round((trade["gst_rs"] + trade["exchange_charges_rs"]
                                 + trade["stamp_duty_rs"] + trade["sebi_tax_rs"]
                                 + trade["other_charges_rs"]) * 100)
        # per-unit brokerage kept as REAL for precision
        brokerage_per_unit = (brokerage_paise / trade["quantity"]) if trade["quantity"] else 0.0
        if trade["side"] == "BUY":
            effective_paise = actual_paise + brokerage_per_unit
        else:
            effective_paise = actual_paise - brokerage_per_unit
        txn_type = trade["side"]  # "BUY" or "SELL"

        conn.execute(
            """INSERT INTO transactions
                   (account_id, instrument_id, txn_type, trade_segment, trade_date,
                    quantity, actual_price_paise, brokerage_per_unit_paise,
                    effective_price_paise, stt_paise, other_charges_paise,
                    broker_ref, batch_id)
               VALUES (?,?,?,?,?,?,?,?,?,?,?,?,?)""",
            (account_id, instrument_id, txn_type, trade["segment"], trade["trade_date"],
             trade["quantity"], actual_paise, brokerage_per_unit,
             effective_paise, stt_paise, other_paise,
             trade["trade_id"] or None, batch_id),
        )
        imported += 1

    conn.commit()
    return {"imported": imported, "skipped": skipped, "auto_created_instruments": auto_created}
