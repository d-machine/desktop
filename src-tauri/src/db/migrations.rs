/// All SQLite migrations in order.
/// Each entry is one migration step — never modify existing entries, only append new ones.
pub const MIGRATIONS: &[&str] = &[
    // -------------------------------------------------------------------------
    // M001 — Core master tables
    // -------------------------------------------------------------------------
    "
    CREATE TABLE exchanges (
        exchange_id   INTEGER PRIMARY KEY,
        code          TEXT NOT NULL UNIQUE,   -- 'NSE', 'BSE', 'MCX'
        name          TEXT NOT NULL,
        country       TEXT NOT NULL DEFAULT 'IN'
    );

    INSERT INTO exchanges (code, name) VALUES
        ('NSE', 'National Stock Exchange'),
        ('BSE', 'Bombay Stock Exchange'),
        ('MCX', 'Multi Commodity Exchange');

    CREATE TABLE instrument_types (
        instrument_type_id  INTEGER PRIMARY KEY,
        name                TEXT NOT NULL UNIQUE,
        asset_class         TEXT NOT NULL,  -- 'EQUITY','MF','FIXED_INCOME','DERIVATIVE','COMMODITY'
        tax_category        TEXT NOT NULL   -- 'EQUITY_LTCG','EQUITY_STCG','DEBT','SPECULATIVE','NON_SPECULATIVE'
    );

    INSERT INTO instrument_types (name, asset_class, tax_category) VALUES
        ('EQUITY',          'EQUITY',       'EQUITY_LTCG'),
        ('EQUITY_MF',       'MF',           'EQUITY_LTCG'),
        ('DEBT_MF',         'MF',           'DEBT'),
        ('HYBRID_MF',       'MF',           'EQUITY_LTCG'),
        ('ELSS',            'MF',           'EQUITY_LTCG'),
        ('FD',              'FIXED_INCOME', 'DEBT'),
        ('BOND',            'FIXED_INCOME', 'DEBT'),
        ('PPF',             'FIXED_INCOME', 'DEBT'),
        ('NPS',             'FIXED_INCOME', 'DEBT'),
        ('FUTURES',         'DERIVATIVE',   'NON_SPECULATIVE'),
        ('OPTIONS',         'DERIVATIVE',   'NON_SPECULATIVE'),
        ('COMMODITY_FUTURES','COMMODITY',   'NON_SPECULATIVE');
    ",

    // -------------------------------------------------------------------------
    // M002 — Instruments base + extension tables
    // -------------------------------------------------------------------------
    "
    CREATE TABLE instruments (
        instrument_id       INTEGER PRIMARY KEY,
        isin                TEXT UNIQUE,
        name                TEXT NOT NULL,
        instrument_type_id  INTEGER NOT NULL REFERENCES instrument_types(instrument_type_id),
        primary_exchange_id INTEGER REFERENCES exchanges(exchange_id),
        is_active           INTEGER NOT NULL DEFAULT 1,
        source              TEXT NOT NULL DEFAULT 'SERVER',  -- 'SERVER','MANUAL'
        created_at          TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at          TEXT NOT NULL DEFAULT (datetime('now'))
    );

    -- Equity extension (v1)
    CREATE TABLE instrument_equity (
        instrument_id       INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        nse_symbol          TEXT,
        bse_code            TEXT,
        face_value_paise    INTEGER,
        sector              TEXT,
        industry            TEXT
    );

    -- Mutual fund extension (v1)
    CREATE TABLE instrument_mf (
        instrument_id   INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        amfi_code       TEXT UNIQUE,
        scheme_type     TEXT,   -- 'EQUITY','DEBT','HYBRID','ELSS','LIQUID', etc.
        fund_house      TEXT,
        plan            TEXT,   -- 'DIRECT','REGULAR'
        option          TEXT    -- 'GROWTH','IDCW'
    );

    -- Fixed income extension (v1)
    CREATE TABLE instrument_fixed_income (
        instrument_id       INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        interest_rate_bps   INTEGER,   -- basis points e.g. 700 = 7.00%
        maturity_date       TEXT,
        compounding         TEXT,      -- 'ANNUAL','QUARTERLY','MONTHLY','CUMULATIVE'
        issuer              TEXT
    );

    -- Derivatives / Futures extension (v2)
    CREATE TABLE instrument_derivatives (
        instrument_id       INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        underlying_isin     TEXT REFERENCES instruments(isin),
        expiry_date         TEXT NOT NULL,
        lot_size            INTEGER NOT NULL,
        strike_price_paise  INTEGER,   -- NULL for futures
        contract_type       TEXT       -- 'FUTURES', 'CE', 'PE'
    );

    -- MCX commodity extension (v3)
    CREATE TABLE instrument_mcx (
        instrument_id   INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        mcx_symbol      TEXT NOT NULL,
        expiry_date     TEXT NOT NULL,
        lot_size        REAL NOT NULL,
        unit            TEXT NOT NULL   -- 'KG','GRAM','BARREL','MMBTU', etc.
    );
    ",

    // -------------------------------------------------------------------------
    // M003 — Portfolios, accounts, import tracking
    // -------------------------------------------------------------------------
    "
    CREATE TABLE portfolios (
        portfolio_id  INTEGER PRIMARY KEY,
        name          TEXT NOT NULL,
        created_at    TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE accounts (
        account_id    INTEGER PRIMARY KEY,
        portfolio_id  INTEGER NOT NULL REFERENCES portfolios(portfolio_id),
        name          TEXT NOT NULL,          -- 'Zerodha Demat', 'HDFC MF Folio'
        account_type  TEXT NOT NULL,          -- 'DEMAT','MF_FOLIO','FD','PPF','NPS','OTHER'
        broker        TEXT,                   -- 'ZERODHA','GROWW','UPSTOX', etc.
        account_no    TEXT,
        created_at    TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE import_batches (
        batch_id      INTEGER PRIMARY KEY,
        account_id    INTEGER NOT NULL REFERENCES accounts(account_id),
        source_type   TEXT NOT NULL,   -- 'CAMS','KFINTECH','ZERODHA','GROWW','MANUAL', etc.
        file_name     TEXT,
        imported_at   TEXT NOT NULL DEFAULT (datetime('now')),
        record_count  INTEGER NOT NULL DEFAULT 0,
        status        TEXT NOT NULL DEFAULT 'COMPLETED',  -- 'COMPLETED','PARTIAL','FAILED'
        notes         TEXT
    );
    ",

    // -------------------------------------------------------------------------
    // M004 — Transactions (source of truth)
    // -------------------------------------------------------------------------
    "
    CREATE TABLE transactions (
        txn_id              INTEGER PRIMARY KEY,
        account_id          INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id       INTEGER NOT NULL REFERENCES instruments(instrument_id),
        txn_type            TEXT NOT NULL,
        -- BUY, SELL, DIVIDEND, INTEREST, BONUS, SPLIT, MERGER_IN, MERGER_OUT,
        -- SWITCH_IN, SWITCH_OUT, SIP, REDEMPTION, OPENING_BALANCE
        trade_date          TEXT NOT NULL,       -- 'YYYY-MM-DD'
        txn_time            TEXT,                -- 'HH:MM:SS' IST — used for intraday matching
        trade_segment       TEXT NOT NULL DEFAULT 'DELIVERY',
        -- 'DELIVERY','INTRADAY','FNO','COMMODITY'
        quantity            REAL NOT NULL,
        price_paise         INTEGER NOT NULL,    -- per unit
        brokerage_paise     INTEGER NOT NULL DEFAULT 0,
        stt_paise           INTEGER NOT NULL DEFAULT 0,
        other_charges_paise INTEGER NOT NULL DEFAULT 0,
        total_value_paise   INTEGER NOT NULL,    -- quantity * price + charges (signed)
        notes               TEXT,
        broker_ref          TEXT,                -- broker's transaction ID for deduplication
        batch_id            INTEGER REFERENCES import_batches(batch_id),
        created_at          TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE INDEX idx_transactions_account ON transactions(account_id);
    CREATE INDEX idx_transactions_instrument ON transactions(instrument_id);
    CREATE INDEX idx_transactions_trade_date ON transactions(trade_date);
    CREATE INDEX idx_transactions_broker_ref ON transactions(broker_ref);
    CREATE UNIQUE INDEX idx_transactions_dedup ON transactions(account_id, broker_ref)
        WHERE broker_ref IS NOT NULL;
    ",

    // -------------------------------------------------------------------------
    // M005 — Market data (local cache, synced from server)
    // -------------------------------------------------------------------------
    "
    CREATE TABLE daily_prices (
        price_id            INTEGER PRIMARY KEY,
        instrument_id       INTEGER NOT NULL REFERENCES instruments(instrument_id),
        trade_date          TEXT NOT NULL,
        open_price_paise    INTEGER,
        high_price_paise    INTEGER,
        low_price_paise     INTEGER,
        close_price_paise   INTEGER NOT NULL,
        volume              INTEGER,
        source              TEXT NOT NULL,   -- 'NSE','BSE','MCX','AMFI'
        UNIQUE(instrument_id, trade_date)
    );

    CREATE INDEX idx_daily_prices_instrument_date ON daily_prices(instrument_id, trade_date);

    CREATE TABLE latest_prices (
        instrument_id       INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        price_date          TEXT NOT NULL,
        close_price_paise   INTEGER NOT NULL,
        updated_at          TEXT NOT NULL DEFAULT (datetime('now'))
    );

    -- MF NAV history (separate from daily_prices — NAV ≠ market price)
    CREATE TABLE nav_history (
        nav_id          INTEGER PRIMARY KEY,
        instrument_id   INTEGER NOT NULL REFERENCES instruments(instrument_id),
        nav_date        TEXT NOT NULL,
        nav_paise       INTEGER NOT NULL,
        UNIQUE(instrument_id, nav_date)
    );

    CREATE INDEX idx_nav_history_instrument_date ON nav_history(instrument_id, nav_date);

    -- 31-Jan-2018 FMV for grandfathering (equity + equity MF)
    CREATE TABLE grandfathering_prices (
        instrument_id   INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        fmv_paise       INTEGER NOT NULL   -- Fair Market Value on 31-Jan-2018
    );
    ",

    // -------------------------------------------------------------------------
    // M006 — Computed tables (rebuilt from transactions)
    // -------------------------------------------------------------------------
    "
    -- Current open positions cache
    CREATE TABLE holdings (
        holding_id          INTEGER PRIMARY KEY,
        portfolio_id        INTEGER NOT NULL,
        account_id          INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id       INTEGER NOT NULL REFERENCES instruments(instrument_id),
        quantity            REAL NOT NULL,
        avg_cost_paise      INTEGER NOT NULL,
        total_cost_paise    INTEGER NOT NULL,
        as_of_date          TEXT NOT NULL,
        UNIQUE(account_id, instrument_id)
    );

    -- Open tax lots (FIFO queue — decremented on sell)
    CREATE TABLE tax_lots (
        lot_id              INTEGER PRIMARY KEY,
        account_id          INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id       INTEGER NOT NULL REFERENCES instruments(instrument_id),
        buy_txn_id          INTEGER NOT NULL REFERENCES transactions(txn_id),
        purchase_date       TEXT NOT NULL,
        original_quantity   REAL NOT NULL,
        remaining_quantity  REAL NOT NULL,
        cost_paise          INTEGER NOT NULL,   -- per unit, including charges
        trade_segment       TEXT NOT NULL DEFAULT 'DELIVERY'
    );

    CREATE INDEX idx_tax_lots_account_instrument ON tax_lots(account_id, instrument_id);

    -- Closed lots (realized capital gains)
    CREATE TABLE closed_lots (
        closed_lot_id       INTEGER PRIMARY KEY,
        portfolio_id        INTEGER NOT NULL,
        account_id          INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id       INTEGER NOT NULL REFERENCES instruments(instrument_id),
        buy_txn_id          INTEGER NOT NULL REFERENCES transactions(txn_id),
        sell_txn_id         INTEGER NOT NULL REFERENCES transactions(txn_id),
        purchase_date       TEXT NOT NULL,
        sell_date           TEXT NOT NULL,
        quantity            REAL NOT NULL,
        buy_price_paise     INTEGER NOT NULL,
        sell_price_paise    INTEGER NOT NULL,
        buy_charges_paise   INTEGER NOT NULL,
        sell_charges_paise  INTEGER NOT NULL,
        gross_pnl_paise     INTEGER NOT NULL,
        net_pnl_paise       INTEGER NOT NULL,
        holding_days        INTEGER NOT NULL,
        gain_type           TEXT NOT NULL,  -- 'LTCG','STCG'
        grandfathered_cost_paise INTEGER,   -- set if purchase_date < 2018-02-01
        financial_year      TEXT NOT NULL   -- '2025-26'
    );

    CREATE INDEX idx_closed_lots_account ON closed_lots(account_id);
    CREATE INDEX idx_closed_lots_fy ON closed_lots(financial_year);

    -- Intraday and F&O P&L (speculative / non-speculative business income)
    CREATE TABLE intraday_pnl (
        pnl_id              INTEGER PRIMARY KEY,
        portfolio_id        INTEGER NOT NULL,
        account_id          INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id       INTEGER NOT NULL REFERENCES instruments(instrument_id),
        buy_txn_id          INTEGER NOT NULL REFERENCES transactions(txn_id),
        sell_txn_id         INTEGER NOT NULL REFERENCES transactions(txn_id),
        trade_date          TEXT NOT NULL,
        quantity            REAL NOT NULL,
        buy_price_paise     INTEGER NOT NULL,
        sell_price_paise    INTEGER NOT NULL,
        gross_pnl_paise     INTEGER NOT NULL,
        charges_paise       INTEGER NOT NULL,
        net_pnl_paise       INTEGER NOT NULL,
        trade_segment       TEXT NOT NULL,   -- 'INTRADAY','FNO','COMMODITY'
        financial_year      TEXT NOT NULL
    );

    CREATE INDEX idx_intraday_pnl_fy ON intraday_pnl(financial_year);

    -- Dividend, interest, and other income events
    CREATE TABLE income_events (
        income_id       INTEGER PRIMARY KEY,
        portfolio_id    INTEGER NOT NULL,
        account_id      INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id   INTEGER NOT NULL REFERENCES instruments(instrument_id),
        txn_id          INTEGER NOT NULL REFERENCES transactions(txn_id),
        income_type     TEXT NOT NULL,   -- 'DIVIDEND','INTEREST','IDCW'
        event_date      TEXT NOT NULL,
        amount_paise    INTEGER NOT NULL,
        tds_paise       INTEGER NOT NULL DEFAULT 0,
        financial_year  TEXT NOT NULL
    );

    -- FY-wise loss carryforward tracker
    CREATE TABLE loss_carryforward (
        loss_id             INTEGER PRIMARY KEY,
        portfolio_id        INTEGER NOT NULL,
        financial_year      TEXT NOT NULL,   -- year loss was incurred '2024-25'
        loss_type           TEXT NOT NULL,   -- 'STCG','LTCG','SPECULATIVE','NON_SPECULATIVE'
        loss_amount_paise   INTEGER NOT NULL,
        setoff_amount_paise INTEGER NOT NULL DEFAULT 0,
        carry_amount_paise  INTEGER NOT NULL,   -- loss_amount - setoff_amount
        expires_fy          TEXT NOT NULL,       -- last year it can be used ('2032-33' for 8yr)
        UNIQUE(portfolio_id, financial_year, loss_type)
    );
    ",

    // -------------------------------------------------------------------------
    // M007 — App configuration
    // -------------------------------------------------------------------------
    "
    CREATE TABLE app_settings (
        key         TEXT PRIMARY KEY,
        value       TEXT NOT NULL,
        updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
    );

    INSERT INTO app_settings (key, value) VALUES
        ('backup_folder_path', ''),
        ('backup_max_count',   '5'),
        ('server_url',         'https://api.portfoliotracker.app'),
        ('last_price_sync',    ''),
        ('active_view_id',     '');

    CREATE TABLE report_views (
        view_id     INTEGER PRIMARY KEY,
        name        TEXT NOT NULL,
        account_ids TEXT NOT NULL,   -- comma-separated e.g. '1,2,4'
        is_default  INTEGER NOT NULL DEFAULT 0,
        created_at  TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE backup_history (
        backup_id       INTEGER PRIMARY KEY,
        file_path       TEXT NOT NULL,
        file_size_bytes INTEGER NOT NULL,
        backed_up_at    TEXT NOT NULL DEFAULT (datetime('now')),
        label           TEXT,
        checksum        TEXT NOT NULL   -- SHA256 of encrypted file
    );

    CREATE TABLE schema_migrations (
        version     INTEGER PRIMARY KEY,
        applied_at  TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE sync_log (
        sync_id     INTEGER PRIMARY KEY,
        sync_type   TEXT NOT NULL,   -- 'PRICES','INSTRUMENTS'
        synced_at   TEXT NOT NULL DEFAULT (datetime('now')),
        status      TEXT NOT NULL,   -- 'SUCCESS','FAILED'
        details     TEXT
    );

    CREATE TABLE corporate_actions (
        action_id       INTEGER PRIMARY KEY,
        instrument_id   INTEGER NOT NULL REFERENCES instruments(instrument_id),
        action_type     TEXT NOT NULL,   -- 'BONUS','SPLIT','MERGER','DIVIDEND'
        ex_date         TEXT NOT NULL,
        ratio_from      REAL,            -- e.g. for 2:1 bonus: ratio_from=1, ratio_to=2
        ratio_to        REAL,
        cash_paise      INTEGER,         -- for special dividends
        notes           TEXT
    );
    ",

    // -------------------------------------------------------------------------
    // M008 — Unique index on instrument_equity.bse_code
    // -------------------------------------------------------------------------
    "
    CREATE UNIQUE INDEX IF NOT EXISTS idx_instrument_equity_bse_code
        ON instrument_equity(bse_code) WHERE bse_code IS NOT NULL;
    ",
];
