import os
import sys
import sqlite3
from pathlib import Path

sys.path.insert(0, '.')
from core.migrations import run_migrations
from importers.bajaj_finance import parse, import_


def setup_db(conn: sqlite3.Connection) -> int:
    run_migrations(conn)
    conn.execute("INSERT INTO persons (name, pan) VALUES (?, ?)", ("Test User", "ABCDE1234F"))
    person_id = conn.execute("SELECT person_id FROM persons WHERE name=?", ("Test User",)).fetchone()[0]
    conn.execute("INSERT INTO portfolios (person_id, name) VALUES (?, ?)", (person_id, "Test Portfolio"))
    portfolio_id = conn.execute("SELECT portfolio_id FROM portfolios WHERE person_id=?", (person_id,)).fetchone()[0]
    conn.execute("INSERT INTO accounts (portfolio_id, name, account_type) VALUES (?, ?, ?)",
                 (portfolio_id, "Test Account", "BROKERAGE"))
    account_id = conn.execute("SELECT account_id FROM accounts WHERE portfolio_id=?", (portfolio_id,)).fetchone()[0]
    conn.commit()
    return account_id


def parse_files(folder: Path, password: str):
    parsed = None
    total = 0
    for fname in sorted(os.listdir(folder)):
        if not fname.lower().endswith('.pdf'):
            continue
        path = folder / fname
        print(f"Parsing {path}")
        result = parse(str(path), password=password)
        print(f"  {len(result['trades'])} trades, CN={result['contract_note_no']}, date={result['trade_date']}")
        if parsed is None:
            parsed = result.copy()
            # keep the first contract note number if only one file
            parsed['trades'] = result['trades'].copy()
        else:
            parsed['trades'].extend(result['trades'])
            parsed['stt_paise'] += result['stt_paise']
            parsed['stamp_paise'] += result['stamp_paise']
            parsed['gst_paise'] += result['gst_paise']
            parsed['exchange_paise'] += result['exchange_paise']
            parsed['other_paise'] += result['other_paise']
            parsed['total_payable_paise'] += result['total_payable_paise']
        total += len(result['trades'])
    print(f"Total trades parsed: {total}")
    return parsed


def main():
    if len(sys.argv) < 3:
        print("Usage: python test_bajaj_import.py <pdf-folder> <password>")
        sys.exit(1)

    folder = Path(sys.argv[1])
    password = sys.argv[2]
    if not folder.exists() or not folder.is_dir():
        raise FileNotFoundError(f"Folder not found: {folder}")

    parsed = parse_files(folder, password)
    if parsed is None:
        raise RuntimeError("No PDF files found to parse")

    conn = sqlite3.connect(':memory:')
    conn.row_factory = sqlite3.Row
    account_id = setup_db(conn)
    result = import_(conn, account_id, parsed, batch_id=1)
    print(f"Import result: {result}")
    rows = list(conn.execute("SELECT txn_id, account_id, broker_ref, effective_price_paise FROM transactions ORDER BY txn_id"))
    print(f"Transactions inserted: {len(rows)}")
    for row in rows[:10]:
        print(dict(row))
    assert result['imported'] == len(parsed['trades']), "Imported count mismatch"
    assert len(rows) == len(parsed['trades']), "DB row count mismatch"
    print("Bajaj import regression test passed")


if __name__ == '__main__':
    main()
