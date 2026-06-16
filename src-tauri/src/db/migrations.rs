/// All SQLite migrations in order.
/// Each entry is one migration step — never modify existing entries, only append new ones.
pub const MIGRATIONS: &[&str] = &[
    // -------------------------------------------------------------------------
    // M001 — Full schema (squashed from original M001–M015)
    // -------------------------------------------------------------------------
    "
    -- ── Master / reference tables ───────────────────────────────────────────

    CREATE TABLE exchanges (
        exchange_id   INTEGER PRIMARY KEY,
        code          TEXT NOT NULL UNIQUE,
        name          TEXT NOT NULL,
        country       TEXT NOT NULL DEFAULT 'IN'
    );

    INSERT INTO exchanges (code, name) VALUES
        ('NSE',  'National Stock Exchange'),
        ('BSE',  'Bombay Stock Exchange'),
        ('MCX',  'Multi Commodity Exchange'),
        ('AMFI', 'Association of Mutual Funds in India');

    -- Server-mastered; populated via GET /instrument-types on first connect.
    -- instrument_type_id uses server canonical IDs directly (not AUTOINCREMENT).
    CREATE TABLE instrument_types (
        instrument_type_id  INTEGER PRIMARY KEY,
        name                TEXT NOT NULL UNIQUE,
        asset_class         TEXT NOT NULL,
        tax_category        TEXT NOT NULL
    );

    -- ── Instruments ─────────────────────────────────────────────────────────

    -- instrument_id is assigned by the server, never auto-generated locally.
    CREATE TABLE instruments (
        instrument_id       INTEGER PRIMARY KEY,
        name                TEXT NOT NULL,
        instrument_type_id  INTEGER NOT NULL REFERENCES instrument_types(instrument_type_id),
        primary_exchange_id INTEGER REFERENCES exchanges(exchange_id),
        is_active           INTEGER NOT NULL DEFAULT 1,
        source              TEXT NOT NULL DEFAULT 'SERVER',
        created_at          TEXT NOT NULL DEFAULT (datetime('now')),
        updated_at          TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE instrument_equity (
        instrument_id       INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        isin                TEXT UNIQUE,
        nse_symbol          TEXT,
        nse_fininstrmid     INTEGER,
        bse_code            TEXT,
        face_value_paise    INTEGER,
        sector              TEXT,
        industry            TEXT
    );

    CREATE UNIQUE INDEX idx_instrument_equity_isin
        ON instrument_equity(isin) WHERE isin IS NOT NULL;
    CREATE UNIQUE INDEX idx_instrument_equity_nse_fininstrmid
        ON instrument_equity(nse_fininstrmid) WHERE nse_fininstrmid IS NOT NULL;
    CREATE UNIQUE INDEX idx_instrument_equity_bse_code
        ON instrument_equity(bse_code) WHERE bse_code IS NOT NULL;
    CREATE UNIQUE INDEX idx_instrument_equity_nse_symbol
        ON instrument_equity(nse_symbol) WHERE nse_symbol IS NOT NULL;

    CREATE TABLE instrument_mf (
        instrument_id   INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        amfi_code       TEXT UNIQUE,
        scheme_type     TEXT,
        fund_house      TEXT,
        plan            TEXT,
        option          TEXT
    );

    CREATE TABLE instrument_fixed_income (
        instrument_id       INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        isin                TEXT UNIQUE,
        interest_rate_bps   INTEGER,
        maturity_date       TEXT,
        compounding         TEXT,
        issuer              TEXT
    );

    CREATE TABLE instrument_derivatives (
        instrument_id            INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        underlying_instrument_id INTEGER REFERENCES instruments(instrument_id),
        underlying_symbol        TEXT NOT NULL,
        expiry_date              TEXT NOT NULL,
        lot_size                 INTEGER NOT NULL,
        strike_price_paise       INTEGER,
        instrument_type          TEXT NOT NULL DEFAULT 'FUTURES',
        option_type              TEXT NOT NULL DEFAULT '-',
        nse_fininstrmid          INTEGER,
        bse_fininstrmid          INTEGER
    );

    CREATE UNIQUE INDEX idx_instrument_derivatives_nse_fininstrmid
        ON instrument_derivatives(nse_fininstrmid) WHERE nse_fininstrmid IS NOT NULL;
    CREATE UNIQUE INDEX idx_instrument_derivatives_bse_fininstrmid
        ON instrument_derivatives(bse_fininstrmid) WHERE bse_fininstrmid IS NOT NULL;

    CREATE TABLE instrument_mcx (
        instrument_id       INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        mcx_symbol          TEXT NOT NULL,
        instrument_type     TEXT,
        expiry_date         TEXT NOT NULL,
        lot_size            REAL NOT NULL,
        unit                TEXT NOT NULL,
        strike_price_paise  INTEGER,
        option_type         TEXT
    );

    CREATE TABLE instrument_index (
        instrument_id   INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        symbol          TEXT NOT NULL,
        exchange        TEXT NOT NULL,
        UNIQUE(symbol, exchange)
    );

    -- Staging table for instruments not yet resolved against the server.
    -- type values: EQUITY | MF | FUTSTK | FUTIDX | OPTSTK | OPTIDX | MCX
    -- metadata keys per type:
    --   EQUITY:          isin, nse_symbol, bse_code, exchange
    --   MF:              isin, amfi_code, amc
    --   FUTSTK/FUTIDX:   underlying_symbol, expiry_date, exchange
    --   OPTSTK/OPTIDX:   underlying_symbol, expiry_date, strike_price_paise, option_type, exchange
    --   MCX:             mcx_symbol, expiry_date, unit
    CREATE TABLE pending_instruments (
        pending_id   INTEGER PRIMARY KEY,
        name         TEXT NOT NULL,
        type         TEXT NOT NULL,
        metadata     TEXT NOT NULL DEFAULT '{}',
        created_at   TEXT NOT NULL DEFAULT (datetime('now'))
    );

    -- ── Persons, portfolios, accounts, import tracking ───────────────────────

    CREATE TABLE persons (
        person_id   INTEGER PRIMARY KEY,
        name        TEXT NOT NULL,
        pan         TEXT,
        created_at  TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE portfolios (
        portfolio_id  INTEGER PRIMARY KEY,
        person_id     INTEGER REFERENCES persons(person_id),
        name          TEXT NOT NULL,
        created_at    TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE accounts (
        account_id    INTEGER PRIMARY KEY,
        portfolio_id  INTEGER NOT NULL REFERENCES portfolios(portfolio_id),
        name          TEXT NOT NULL,
        account_type  TEXT NOT NULL,
        broker        TEXT,
        account_no    TEXT,
        created_at    TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE import_batches (
        batch_id            INTEGER PRIMARY KEY,
        account_id          INTEGER NOT NULL REFERENCES accounts(account_id),
        source_type         TEXT NOT NULL,
        file_name           TEXT,
        ref_no              TEXT,
        broker              TEXT,
        batch_trade_date    TEXT,
        imported_at         TEXT NOT NULL DEFAULT (datetime('now')),
        record_count        INTEGER NOT NULL DEFAULT 0,
        status              TEXT NOT NULL DEFAULT 'COMPLETED',
        notes               TEXT,
        stt_paise           INTEGER NOT NULL DEFAULT 0,
        stamp_charges_paise INTEGER NOT NULL DEFAULT 0,
        gst_paise           INTEGER NOT NULL DEFAULT 0,
        trans_charges_paise INTEGER NOT NULL DEFAULT 0,
        other_charges_paise INTEGER NOT NULL DEFAULT 0,
        total_payable_paise INTEGER NOT NULL DEFAULT 0
    );

    -- ── Transactions (source of truth) ──────────────────────────────────────
    --
    -- Exactly one of instrument_id / pending_instrument_id must be non-NULL.
    -- flag values: OVERSELL | UNMATCHED_INSTRUMENT | DUPLICATE_SUSPECTED | USER_FLAGGED
    -- Flagged + not dismissed → excluded from FIFO / portfolio calculations.

    CREATE TABLE transactions (
        txn_id                INTEGER PRIMARY KEY,
        account_id            INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id         INTEGER REFERENCES instruments(instrument_id),
        pending_instrument_id INTEGER REFERENCES pending_instruments(pending_id),
        txn_type              TEXT NOT NULL,
        trade_date            TEXT NOT NULL,
        txn_time              TEXT,
        trade_segment         TEXT NOT NULL DEFAULT 'DELIVERY',
        quantity              REAL NOT NULL,
        price_paise           INTEGER NOT NULL,
        brokerage_paise       INTEGER NOT NULL DEFAULT 0,
        stt_paise             INTEGER NOT NULL DEFAULT 0,
        other_charges_paise   INTEGER NOT NULL DEFAULT 0,
        total_value_paise     INTEGER NOT NULL,
        notes                 TEXT,
        broker_ref            TEXT,
        batch_id              INTEGER REFERENCES import_batches(batch_id),
        created_at            TEXT NOT NULL DEFAULT (datetime('now')),
        flag                  TEXT    DEFAULT NULL,
        flag_reason           TEXT    DEFAULT NULL,
        flag_dismissed        INTEGER NOT NULL DEFAULT 0,
        CHECK (
            (instrument_id IS NOT NULL AND pending_instrument_id IS NULL) OR
            (instrument_id IS NULL     AND pending_instrument_id IS NOT NULL)
        )
    );

    CREATE INDEX idx_transactions_account     ON transactions(account_id);
    CREATE INDEX idx_transactions_instrument  ON transactions(instrument_id)
        WHERE instrument_id IS NOT NULL;
    CREATE INDEX idx_transactions_pending     ON transactions(pending_instrument_id)
        WHERE pending_instrument_id IS NOT NULL;
    CREATE INDEX idx_transactions_trade_date  ON transactions(trade_date);
    CREATE INDEX idx_transactions_broker_ref  ON transactions(broker_ref);
    CREATE UNIQUE INDEX idx_transactions_dedup ON transactions(account_id, broker_ref)
        WHERE broker_ref IS NOT NULL;

    -- ── Market data (local price cache, synced from server) ─────────────────

    CREATE TABLE daily_prices (
        price_id            INTEGER PRIMARY KEY,
        instrument_id       INTEGER NOT NULL REFERENCES instruments(instrument_id),
        trade_date          TEXT NOT NULL,
        open_price_paise    INTEGER,
        high_price_paise    INTEGER,
        low_price_paise     INTEGER,
        close_price_paise   INTEGER NOT NULL,
        volume              INTEGER,
        source              TEXT NOT NULL,
        UNIQUE(instrument_id, trade_date)
    );

    CREATE INDEX idx_daily_prices_instrument_date ON daily_prices(instrument_id, trade_date);

    CREATE TABLE latest_prices (
        instrument_id       INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        price_date          TEXT NOT NULL,
        open_price_paise    INTEGER,
        high_price_paise    INTEGER,
        low_price_paise     INTEGER,
        close_price_paise   INTEGER NOT NULL,
        updated_at          TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE TABLE nav_history (
        nav_id          INTEGER PRIMARY KEY,
        instrument_id   INTEGER NOT NULL REFERENCES instruments(instrument_id),
        nav_date        TEXT NOT NULL,
        nav_paise       INTEGER NOT NULL,
        UNIQUE(instrument_id, nav_date)
    );

    CREATE INDEX idx_nav_history_instrument_date ON nav_history(instrument_id, nav_date);

    CREATE TABLE grandfathering_prices (
        instrument_id   INTEGER PRIMARY KEY REFERENCES instruments(instrument_id),
        fmv_paise       INTEGER NOT NULL
    );

    -- ── Computed tables (rebuilt from transactions on demand) ────────────────

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

    CREATE TABLE tax_lots (
        lot_id              INTEGER PRIMARY KEY,
        account_id          INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id       INTEGER NOT NULL REFERENCES instruments(instrument_id),
        buy_txn_id          INTEGER NOT NULL REFERENCES transactions(txn_id),
        purchase_date       TEXT NOT NULL,
        original_quantity   REAL NOT NULL,
        remaining_quantity  REAL NOT NULL,
        cost_paise          INTEGER NOT NULL,
        trade_segment       TEXT NOT NULL DEFAULT 'DELIVERY'
    );

    CREATE INDEX idx_tax_lots_account_instrument ON tax_lots(account_id, instrument_id);

    CREATE TABLE closed_lots (
        closed_lot_id           INTEGER PRIMARY KEY,
        portfolio_id            INTEGER NOT NULL,
        account_id              INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id           INTEGER NOT NULL REFERENCES instruments(instrument_id),
        buy_txn_id              INTEGER NOT NULL REFERENCES transactions(txn_id),
        sell_txn_id             INTEGER NOT NULL REFERENCES transactions(txn_id),
        purchase_date           TEXT NOT NULL,
        sell_date               TEXT NOT NULL,
        quantity                REAL NOT NULL,
        buy_price_paise         INTEGER NOT NULL,
        sell_price_paise        INTEGER NOT NULL,
        buy_charges_paise       INTEGER NOT NULL,
        sell_charges_paise      INTEGER NOT NULL,
        gross_pnl_paise         INTEGER NOT NULL,
        net_pnl_paise           INTEGER NOT NULL,
        holding_days            INTEGER NOT NULL,
        gain_type               TEXT NOT NULL,
        grandfathered_cost_paise INTEGER,
        financial_year          TEXT NOT NULL
    );

    CREATE INDEX idx_closed_lots_account ON closed_lots(account_id);
    CREATE INDEX idx_closed_lots_fy ON closed_lots(financial_year);

    CREATE TABLE intraday_pnl (
        pnl_id          INTEGER PRIMARY KEY,
        portfolio_id    INTEGER NOT NULL,
        account_id      INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id   INTEGER NOT NULL REFERENCES instruments(instrument_id),
        buy_txn_id      INTEGER NOT NULL REFERENCES transactions(txn_id),
        sell_txn_id     INTEGER NOT NULL REFERENCES transactions(txn_id),
        trade_date      TEXT NOT NULL,
        quantity        REAL NOT NULL,
        buy_price_paise INTEGER NOT NULL,
        sell_price_paise INTEGER NOT NULL,
        gross_pnl_paise INTEGER NOT NULL,
        charges_paise   INTEGER NOT NULL,
        net_pnl_paise   INTEGER NOT NULL,
        trade_segment   TEXT NOT NULL,
        financial_year  TEXT NOT NULL
    );

    CREATE INDEX idx_intraday_pnl_fy ON intraday_pnl(financial_year);

    CREATE TABLE income_events (
        income_id       INTEGER PRIMARY KEY,
        portfolio_id    INTEGER NOT NULL,
        account_id      INTEGER NOT NULL REFERENCES accounts(account_id),
        instrument_id   INTEGER NOT NULL REFERENCES instruments(instrument_id),
        txn_id          INTEGER NOT NULL REFERENCES transactions(txn_id),
        income_type     TEXT NOT NULL,
        event_date      TEXT NOT NULL,
        amount_paise    INTEGER NOT NULL,
        tds_paise       INTEGER NOT NULL DEFAULT 0,
        financial_year  TEXT NOT NULL
    );

    CREATE TABLE loss_carryforward (
        loss_id             INTEGER PRIMARY KEY,
        portfolio_id        INTEGER NOT NULL,
        financial_year      TEXT NOT NULL,
        loss_type           TEXT NOT NULL,
        loss_amount_paise   INTEGER NOT NULL,
        setoff_amount_paise INTEGER NOT NULL DEFAULT 0,
        carry_amount_paise  INTEGER NOT NULL,
        expires_fy          TEXT NOT NULL,
        UNIQUE(portfolio_id, financial_year, loss_type)
    );

    -- ── App configuration ────────────────────────────────────────────────────

    CREATE TABLE app_settings (
        key         TEXT PRIMARY KEY,
        value       TEXT NOT NULL,
        updated_at  TEXT NOT NULL DEFAULT (datetime('now'))
    );

    INSERT INTO app_settings (key, value) VALUES
        ('backup_folder_path',    ''),
        ('backup_max_count',      '5'),
        ('server_url',            'https://arthdeskapi.ashokitservices.com'),
        ('last_price_sync',       ''),
        ('last_instrument_sync',  ''),
        ('active_view_id',        '');

    CREATE TABLE report_views (
        view_id     INTEGER PRIMARY KEY,
        name        TEXT NOT NULL,
        account_ids TEXT NOT NULL,
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
        checksum        TEXT NOT NULL
    );

    CREATE TABLE sync_log (
        sync_id     INTEGER PRIMARY KEY,
        sync_type   TEXT NOT NULL,
        synced_at   TEXT NOT NULL DEFAULT (datetime('now')),
        status      TEXT NOT NULL,
        details     TEXT
    );

    -- ── Corporate actions audit log ──────────────────────────────────────────
    --
    -- One row per compound operation (TRANSFER, SPLIT, BONUS, MERGER, etc.).
    -- The transactions table drives all holdings computation; this table is
    -- purely for audit / linkage / undo support.
    --
    -- JSON shapes:
    --   TRANSFER: { type, transfer_date, from_account_id, to_account_id,
    --               instrument_id, quantity, price_paise,
    --               transfer_out_txn_id, transfer_in_txn_id }
    --   SPLIT:    { type, ex_date, account_id, from_instrument_id,
    --               to_instrument_id, qty_before, qty_after,
    --               avg_cost_before_paise, split_out_txn_id, split_in_txn_id }

    CREATE TABLE corporate_actions (
        ca_id      INTEGER PRIMARY KEY,
        data       TEXT NOT NULL DEFAULT '{}',
        created_at TEXT NOT NULL DEFAULT (datetime('now'))
    );

    -- ── Charges (manual + import-sourced trading costs) ──────────────────────

    CREATE TABLE charges (
        charge_id       INTEGER PRIMARY KEY,
        account_id      INTEGER NOT NULL REFERENCES accounts(account_id),
        start_date      TEXT NOT NULL,
        end_date        TEXT NOT NULL,
        charge_type     TEXT NOT NULL,
        amount_paise    INTEGER NOT NULL,
        source          TEXT NOT NULL DEFAULT 'MANUAL',
        import_batch_id INTEGER REFERENCES import_batches(batch_id),
        notes           TEXT,
        created_at      TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE INDEX idx_charges_account ON charges(account_id);
    CREATE INDEX idx_charges_dates   ON charges(start_date, end_date);
    ",

    // -------------------------------------------------------------------------
    // M002 — Tax ledger
    // -------------------------------------------------------------------------
    "
    -- Person-level tax entries: TDS withheld on dividends/interest,
    -- advance tax payments, self-assessment tax, etc.
    -- txn_id is optional — links to the source transaction when known.
    CREATE TABLE tax_entries (
        entry_id     INTEGER PRIMARY KEY,
        person_id    INTEGER NOT NULL REFERENCES persons(person_id),
        entry_type   TEXT NOT NULL CHECK(entry_type IN ('TDS','ADVANCE_TAX','SELF_ASSESSMENT_TAX')),
        amount_paise INTEGER NOT NULL,
        entry_date   TEXT NOT NULL,
        fy           TEXT NOT NULL,
        txn_id       INTEGER REFERENCES transactions(txn_id),
        notes        TEXT,
        created_at   TEXT NOT NULL DEFAULT (datetime('now'))
    );

    CREATE INDEX idx_tax_entries_person ON tax_entries(person_id);
    CREATE INDEX idx_tax_entries_fy     ON tax_entries(person_id, fy);
    ",
];
