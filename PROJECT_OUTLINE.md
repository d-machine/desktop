# Portfolio Tracker — Project Outline & PyQt6 Architecture

## What This App Does

**Portfolio Tracker** is a privacy-first, offline-capable desktop application for Indian retail investors to track their investments across multiple brokers, asset classes, and accounts — all stored locally in an encrypted SQLite database.

### Core Purpose
- Track investments across **Equity, Mutual Funds, Fixed Income, Derivatives, Commodities**
- Calculate **current holdings** using FIFO lot matching (with intraday netting)
- Generate **capital gains reports** (STCG/LTCG) aligned with Indian tax law
- Import broker statements automatically (Angel One, CAMS CAS, Choice MF/Equity, ICICI Securities)
- Sync **live market prices** from a backend server
- Everything is **encrypted at rest** — only accessible via a PIN

---

## Features

### F1 — Authentication
- **First-time setup**: Generate a random 32-byte master key. Wrap it with Argon2id-derived key from PIN → stored in `auth.json`. Wrap again with passphrase → returned as `.ptbak` recovery file.
- **Login**: Enter PIN → derive key → unwrap master key → open encrypted SQLite DB.
- **Lock**: Drop DB connection, clear master key from memory.
- **Recovery**: Load `.ptbak` file → enter passphrase → unwrap master key → set new PIN → re-wrap.
- **Change PIN**: Unwrap with old PIN → re-wrap with new PIN.

### F2 — Portfolios & Accounts
- **Portfolios**: Named groups (e.g. "Family", "Trading"). CRUD operations.
- **Accounts**: Belong to a portfolio. Represent broker accounts (e.g. "Angel One - Zerodha"). CRUD operations.
- **Sidebar selector**: Switch active portfolio; filter all views by portfolio.

### F3 — Instruments
- Instruments have a type (`EQUITY`, `EQUITY_MF`, `DEBT_MF`, `HYBRID_MF`, `ELSS`, `FD`, `BOND`, `PPF`, `NPS`, `FUTURES`, `OPTIONS`, `COMMODITY_FUTURES`).
- Each type has an `asset_class` and `tax_category`.
- Extension tables store type-specific data (NSE symbol, ISIN, AMFI code, lot size, expiry, etc.).
- **Search**: Full-text search across name, ISIN, symbol.
- **Create manually**: For FD, Bonds, PPF, etc.
- **Server resolution**: Match local instruments to server canonical records via ISIN/symbol.

### F4 — Transactions
All financial events are stored as transactions with these types:
- **Buy-side**: `BUY`, `SIP`, `OPENING_BALANCE`, `BONUS`, `MERGER_IN`, `SWITCH_IN`, `TRANSFER_IN`
- **Sell-side**: `SELL`, `REDEMPTION`, `MERGER_OUT`, `SWITCH_OUT`, `TRANSFER_OUT`
- **Income**: `DIVIDEND`, `INTEREST`
- **Charges**: `FUTURES`, `OPTIONS` (intraday/speculative)
- Trade segments: `DELIVERY`, `INTRADAY`, `FUTURES`, `OPTIONS`
- Charges tracked: brokerage, STT, other charges
- **Flagging**: Transactions that create an oversell (sell before buy) are automatically flagged with `OVERSELL`. User can dismiss flags.
- **Transfer**: Move holdings between accounts (creates TRANSFER_OUT + TRANSFER_IN pair).
- **Import Batches**: Transactions imported together are grouped in a `batch_id`.

### F5 — Holdings (FIFO Engine)
- Compute current open positions by running FIFO lot matching on all transactions.
- **Intraday netting**: Intraday BUY/SELL pairs are netted — only the delivery imbalance enters the FIFO.
- Output per position: quantity, avg cost, total cost, current price, current value, unrealized P&L (absolute + %).
- **Portfolio Summary**: Total invested, current value, unrealized P&L, holdings count.

### F6 — Dashboard
- Portfolio summary cards (invested, current value, P&L).
- Asset allocation donut chart (Equity, MF, Fixed Income, etc.).
- Recent transactions list.
- Capital gains summary table (current FY).

### F7 — Price Sync
- **Resolve instruments**: Send local instruments (ISIN/symbol) to the server → receive canonical mappings → update local DB.
- **Sync prices**: Fetch latest close prices for all held instruments from server → store in `latest_prices` table.
- Sync status shown in navbar.

### F8 — Capital Gains Report
- FIFO-based calculation across all sell transactions.
- Categorized by:
  - **STCG / LTCG** (Indian holding period rules: <12 months = STCG for equity, <24 months for debt)
  - **Equity vs Debt**
  - **Speculative** (intraday equity)
  - **Non-Speculative** (F&O)
- Per Indian **Financial Year** (April–March).
- Summary table + detailed lot-by-lot view.
- **Excel export**: 3-sheet workbook (CG Summary, CG Details, Income).

### F9 — Income Report
- Track `DIVIDEND` and `INTEREST` transactions per FY.
- Summary by FY with dividend/interest breakdown.

### F10 — Asset Allocation Page
- Portfolio breakdown by asset class with donut chart and table.
- Filter by portfolio.

### F11 — Import Parsers
Supported broker statement formats:
1. **Angel One** — `.xlsx` trade & charges file from back-office
2. **CAMS CAS** — `.pdf` Consolidated Account Statement (all AMCs)
3. **Choice Wealth MF** — `.pdf` mutual fund statement
4. **Choice Equity** — `.pdf` Global Details Report
5. **ICICI Securities** — `.pdf` equity transaction report

Each importer: parse → preview with parsed transactions → confirm → import into DB → run oversell flag check.

### F12 — Backup & Restore
- **Export**: Copy the encrypted DB file to a user-chosen location.
- **Import**: Replace the current DB with a backup file (reinitializes connection).

### F13 — Settings
- App settings stored as key-value pairs in the DB.
- Change PIN.
- Server URL configuration.
- Currency display preference.

---

## Database Schema (Key Tables)

```
exchanges           — NSE, BSE, MCX
instrument_types    — EQUITY, EQUITY_MF, DEBT_MF, etc. with asset_class + tax_category
instruments         — Core instrument record (ISIN, name, type)
instrument_equity   — NSE/BSE symbol, sector
instrument_mf       — AMFI code, fund house, plan, option
instrument_fixed_income — interest rate, maturity, issuer
instrument_derivatives  — underlying, expiry, lot size, strike
instrument_mcx      — MCX symbol
portfolios          — named portfolio groups
accounts            — broker accounts (belong to a portfolio)
transactions        — all financial events (buy/sell/income/etc.)
import_batches      — groups of imported transactions
latest_prices       — most recent close price per instrument
settings            — key-value store
```

---

## PyQt6 Architecture

### Design Principle: Strict Frontend / Backend Separation

The app is divided into two layers connected only through a well-defined Python interface. The **backend** can be swapped from local SQLite to a cloud API without touching any frontend code.

```
pyqt6_app/
│
├── main.py                        # Entry point — creates QApplication, wires backend, shows window
├── requirements.txt
│
├── backend/
│   ├── __init__.py
│   ├── interface.py               # Abstract base class — defines ALL backend operations
│   ├── models.py                  # Pure Python dataclasses (shared by frontend & backend)
│   │
│   ├── local/                     # Local SQLite backend (current default)
│   │   ├── __init__.py
│   │   ├── backend.py             # Implements BackendInterface
│   │   ├── auth.py                # PIN/key management (Argon2, AES-GCM)
│   │   ├── db.py                  # SQLite connection management
│   │   ├── migrations.py          # Schema migrations array
│   │   ├── portfolio.py           # Portfolio & account CRUD
│   │   ├── instrument.py          # Instrument search & CRUD
│   │   ├── transaction.py         # Transaction CRUD + flag logic
│   │   ├── holdings.py            # FIFO engine + intraday netting
│   │   ├── prices.py              # Resolve instruments + sync prices (HTTP)
│   │   ├── reports.py             # Capital gains + income + Excel export
│   │   ├── settings.py            # Settings key-value store
│   │   ├── backup.py              # Export / import DB backup
│   │   └── importers/
│   │       ├── __init__.py
│   │       ├── angel_one.py       # Angel One XLSX parser
│   │       ├── cams_cas.py        # CAMS CAS PDF parser
│   │       ├── choice_mf.py       # Choice Wealth MF PDF parser
│   │       ├── choice_equity.py   # Choice Equity PDF parser
│   │       └── icici_equity.py    # ICICI Securities PDF parser
│   │
│   └── api/                       # (Future) Cloud API backend
│       ├── __init__.py
│       └── backend.py             # Implements BackendInterface via HTTP calls
│
└── frontend/
    ├── __init__.py
    ├── main_window.py             # QMainWindow — holds sidebar + page stack
    ├── styles.py                  # QSS stylesheet constants
    ├── utils.py                   # Format helpers (INR, dates, qty)
    │
    ├── auth/
    │   ├── login_screen.py        # PIN entry screen
    │   ├── setup_screen.py        # First-time PIN + passphrase setup
    │   └── forgot_pin_dialog.py   # Recovery flow
    │
    ├── widgets/
    │   ├── sidebar.py             # Left navigation sidebar
    │   ├── navbar.py              # Top bar (portfolio selector, sync status, lock button)
    │   ├── pin_input.py           # Custom PIN dot-input widget
    │   └── charts.py             # Reusable chart widgets (donut, bar)
    │
    ├── pages/
    │   ├── dashboard.py           # Dashboard page
    │   ├── holdings.py            # Holdings table page
    │   ├── transactions.py        # Transactions table page
    │   ├── asset_allocation.py    # Asset allocation page
    │   ├── capital_gains.py       # Capital gains report page
    │   ├── income.py              # Income report page
    │   └── settings.py            # Settings page
    │
    └── dialogs/
        ├── add_transaction.py     # Add transaction dialog
        ├── edit_transaction.py    # Edit transaction dialog
        ├── import_dialog.py       # Import broker statement dialog
        ├── transfer_dialog.py     # Transfer holding between accounts
        ├── onboarding_wizard.py   # First-run onboarding wizard
        └── backup_dialog.py       # Export / import backup dialogs
```

### Backend Interface Contract (`backend/interface.py`)

```python
class BackendInterface(ABC):
    # Auth
    def is_setup(self) -> bool: ...
    def is_unlocked(self) -> bool: ...
    def setup(self, pin: str, passphrase: str) -> str: ...     # returns recovery JSON
    def login(self, pin: str) -> None: ...
    def lock(self) -> None: ...
    def recover(self, recovery_json: str, new_pin: str) -> None: ...
    def change_pin(self, old_pin: str, new_pin: str) -> None: ...

    # Portfolios
    def get_portfolios(self) -> List[Portfolio]: ...
    def create_portfolio(self, name: str) -> Portfolio: ...
    def rename_portfolio(self, id: int, name: str) -> None: ...
    def delete_portfolio(self, id: int) -> None: ...

    # Accounts
    def get_accounts(self, portfolio_id: Optional[int]) -> List[Account]: ...
    def create_account(self, portfolio_id: int, name: str, broker: str) -> Account: ...
    def rename_account(self, id: int, name: str) -> None: ...
    def delete_account(self, id: int) -> None: ...

    # Instruments
    def search_instruments(self, query: str, type_filter: Optional[str]) -> List[Instrument]: ...
    def get_instrument_types(self) -> List[InstrumentType]: ...
    def create_instrument(self, ...) -> Instrument: ...

    # Transactions
    def get_transactions(self, filter: TransactionFilter) -> List[Transaction]: ...
    def create_transaction(self, input: CreateTransactionInput) -> Transaction: ...
    def update_transaction(self, input: UpdateTransactionInput) -> Transaction: ...
    def delete_transaction(self, txn_id: int) -> None: ...
    def dismiss_transaction_flag(self, txn_id: int) -> None: ...
    def re_evaluate_flags(self, account_id: int) -> None: ...
    def transfer_holding(self, ...) -> None: ...
    def get_import_batch(self, batch_id: int) -> List[Transaction]: ...

    # Holdings
    def get_holdings(self, ...) -> List[Holding]: ...
    def get_portfolio_summary(self, ...) -> PortfolioSummary: ...

    # Prices
    def resolve_instruments(self) -> ResolveResult: ...
    def sync_prices(self) -> SyncPricesResult: ...

    # Reports
    def get_capital_gains(self, fy: Optional[str], account_ids: Optional[List[int]]) -> CapitalGainsReport: ...
    def get_income(self, fy: Optional[str], account_ids: Optional[List[int]]) -> IncomeReport: ...
    def export_tax_report(self, fy: str, path: str) -> None: ...

    # Import
    def get_import_sources(self) -> List[ImportSource]: ...
    def parse_statement(self, source: str, file_path: str) -> List[ParsedTransaction]: ...
    def import_statement(self, source: str, account_id: int, parsed: List[ParsedTransaction]) -> int: ...

    # Settings
    def get_setting(self, key: str) -> Optional[str]: ...
    def set_setting(self, key: str, value: str) -> None: ...

    # Backup
    def export_data(self, dest_path: str) -> None: ...
    def import_data(self, src_path: str) -> None: ...
```

### Key Design Decisions

1. **Backend is a singleton** injected into `main.py`. Frontend widgets receive it via constructor.
2. **All backend calls are synchronous** for simplicity. Long operations (price sync, parsing) run in `QThread` workers.
3. **No Tauri IPC layer** — direct Python method calls replace the JS → Rust invoke bridge.
4. **Paise everywhere** — all monetary values internally stored as integers (1 rupee = 100 paise).
5. **Swapping backend**: Change one line in `main.py` — `backend = LocalBackend(app_dir)` → `backend = ApiBackend(server_url, token)`.

---

## Implementation Order

| # | Feature | Status |
|---|---------|--------|
| 1 | Project skeleton + backend interface + models | pending |
| 2 | Auth (PIN setup, login, lock, recovery) | pending |
| 3 | Portfolio & Account management | pending |
| 4 | Transactions (CRUD, flagging) | pending |
| 5 | Holdings (FIFO engine) | pending |
| 6 | Dashboard | pending |
| 7 | Price sync | pending |
| 8 | Capital Gains & Income reports | pending |
| 9 | Import parsers | pending |
| 10 | Backup / Restore | pending |
| 11 | Settings | pending |
