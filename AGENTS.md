# Arthdesk — Agent Context

## Project Overview
Desktop app for Indian portfolio tracking. Built with Tauri + React + Python FastAPI.

## Architecture
- **Frontend**: React + TypeScript + Tailwind (shadcn/ui), in `src/`
- **Backend**: Python FastAPI sidecar in `backend/`, SQLite database encrypted at rest
- **Tauri**: Shell only — launches Python sidecar, handles file dialogs, native OS APIs
- **API**: `src/lib/api.ts` — all calls use `apiPost`/`apiGet` to `http://localhost:{port}/api/...`
- **Auth**: Session token sent as `X-Session-Token` header on every request
- **DB encryption**: AES-256-GCM full-file encryption at rest (`portfolio.db.enc`), plaintext while running

## Key Backend Files
```
backend/
├── main.py                          — FastAPI app, lifespan, CLI args (--port, --db-path)
├── core/
│   ├── auth.py                      — Argon2id key derivation, AES-GCM wrap/unwrap
│   ├── db.py                        — SQLite open/close + AES-GCM file encryption
│   ├── migrations.py                — All DDL (M001–M004), append-only
│   └── session.py                   — In-memory session token → master_key store
├── routers/
│   ├── deps.py                      — get_conn: opens per-request SQLite connection (NOT shared)
│   ├── auth.py, persons.py, portfolios.py, accounts.py, settings.py
│   ├── instruments.py, transactions.py, holdings.py
│   ├── reports.py, charges.py, tax.py
│   ├── import_.py, backup.py, prices.py
├── services/
│   ├── holdings.py                  — FIFO engine + intraday netting
│   ├── reports.py                   — Capital gains FIFO, income, Excel export
│   ├── flags.py                     — Oversell detection
│   └── prices.py                    — Price sync from external server
└── importers/
    ├── bajaj_finance.py             — Bajaj Broking contract note PDF parser
    ├── angel_one.py                 — Angel One XLSX parser
    ├── invest_plus_opening_stock.py — Invest Plus opening stock XLS parser
    ├── cams_cas.py, choice_mf.py, ce_global.py
    ├── icici_equity.py, cn_choice_equity.py, cn_woodstock.py, cn_nirmal_bang.py
    └── pdf_utils.py                 — open_pdf helper (pdfplumber + pypdf fallback)
```

## Transaction Price Schema (M003 + M004)
Three per-unit price fields, all `REAL` (not INTEGER) to preserve 4 decimal places:
- `actual_price_paise REAL` — gross rate per unit (nullable when unknown)
- `brokerage_per_unit_paise REAL` — per-unit brokerage (nullable when unknown)
- `effective_price_paise REAL NOT NULL` — all-in rate; sole source of truth for cost/gain calculations

`total_value_paise` was **dropped** in M003 — computed on the fly as `qty × effective_price_paise`.

Prices stored as REAL paise (e.g. 42.1645 ₹ → 4216.45 paise) to preserve sub-paise precision from contract notes. Total/charge columns (stt_paise, other_charges_paise, etc.) remain INTEGER.

### Bajaj Finance price mapping
- `actual_price_paise` = gross WAP (col 4/9 in summary table)
- `brokerage_per_unit_paise` = exchange charges per unit (col 5/10)
- `effective_price_paise` = net price (col 6/11) — gross ± exchange charges

### Intraday rule
Parsers emit DELIVERY only. Intraday = min(buy, sell) per ISIN per day, computed at reporting time in `services/reports.py` and `services/holdings.py`.

## Capital Gains
- Uses `effective_price_paise` for both buy cost and sell proceeds
- FIFO delivery matching per (account, instrument)
- Same-day netting first, then delivery FIFO
- Gain types: STCG/LTCG (equity ≥365d, debt ≥1095d), SPECULATIVE (intraday equity), NON_SPECULATIVE (F&O)

## Connection Handling
`backend/routers/deps.py` `get_conn` opens a **fresh per-request connection** (generator dependency).
This fixed a `sqlite3.InterfaceError: bad parameter or other API misuse` error caused by 15 concurrent
parse requests sharing one connection. `app.state.conn` is still set on login and used only by the
`lock` endpoint for checkpoint + encrypt. `app.state.db_path` stores the plaintext DB path for
per-request connections.

## Holdings Page UI (`src/pages/HoldingsPage.tsx`)
- **Grid**: `grid-cols-[minmax(220px,1fr)_80px_104px_140px_96px_140px_148px_64px]`
- **Borders**: `border-slate-400 dark:border-slate-500` on all cells (traditional table grid look)
- **Outer card**: `border border-slate-400 dark:border-slate-500 rounded-lg overflow-clip`
- **Sticky headers**: group header + column header wrapped together in one `sticky top-0 z-10` div
- **Last row**: no `border-b` — controlled via `isLast` prop on `HoldingRow`
- **Group spacing**: first group has no top margin, subsequent groups get `mt-2` via `className` prop
- **Scroll container**: `flex-1 overflow-auto` div wrapping all `AssetGroup` components

## Pending Work
- **Re-import Bajaj Finance files** — existing records have old rounded integer prices; delete and
  re-import to get REAL precision
- **Capital gains detail view UI** (`src/pages/CapitalGainsPage.tsx`) — lot-level rows grouped by
  instrument with subtotals, matching Invest Plus screenshot style
- **Excel export** (`services/reports.py` `export_tax_report`) — same grouped layout
- **Master migration plan** (Rust→Python, Tauri sidecar, React invoke→fetch, PyInstaller):
  `C:\Users\SUMIT\.claude\plans\let-s-create-a-plan-lively-glacier.md`
- **Transaction price schema plan** (M003/M004 redesign details):
  `C:\Users\SUMIT\.claude\plans\transaction-price-schema.md`

## Frontend API Pattern
```typescript
import { apiPost, apiGet } from "@/lib/api";
const result = await apiPost<ReturnType>("/endpoint", { ...body });
```

## Import Flow
1. `POST /api/import/parse` — parse file, returns preview data (no DB writes)
2. `POST /api/import/confirm` — write to DB, returns `{ batch_id, imported, skipped }`
- Password auto-try order: saved password → account PAN → prompt user
- PDF password errors return 422 with detail `"WRONG_PASSWORD"` → frontend shows password prompt
