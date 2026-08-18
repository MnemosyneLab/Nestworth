CREATE TABLE instruments (
    id                  TEXT PRIMARY KEY NOT NULL,
    household_id        TEXT NOT NULL,

    name                TEXT NOT NULL,
    symbol              TEXT,

    instrument_type     TEXT NOT NULL
        CHECK(instrument_type IN (
            'stock',
            'etf',
            'mutual_fund',
            'crypto',
            'bond',
            'precious_metal',
            'bank_investment_product',
            'other'
        )),

    quote_currency      TEXT NOT NULL
        CHECK(quote_currency GLOB '[A-Z][A-Z][A-Z]'),

    market_code         TEXT,
    country_code        TEXT,
    isin                TEXT,

    provider_key        TEXT,
    provider_symbol     TEXT,

    quote_preference    TEXT NOT NULL
        CHECK(quote_preference IN ('manual', 'provider')),

    note                TEXT,
    logo_asset_id       TEXT,

    sort_order          INTEGER NOT NULL DEFAULT 0,

    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    archived_at         TEXT,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE,

    FOREIGN KEY(logo_asset_id)
        REFERENCES media_assets(id)
        ON DELETE SET NULL,

    CHECK(
        (provider_key IS NULL AND provider_symbol IS NULL)
        OR (provider_key IS NOT NULL AND provider_symbol IS NOT NULL)
    )
);

CREATE INDEX idx_instruments_household
ON instruments(household_id);

CREATE UNIQUE INDEX idx_instruments_provider_identity
ON instruments(household_id, provider_key, provider_symbol)
WHERE provider_key IS NOT NULL AND provider_symbol IS NOT NULL;

CREATE TABLE holdings (
    id              TEXT PRIMARY KEY NOT NULL,
    account_id      TEXT NOT NULL,
    instrument_id   TEXT NOT NULL,

    quantity        TEXT NOT NULL,
    note            TEXT,

    sort_order      INTEGER NOT NULL DEFAULT 0,

    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    archived_at     TEXT,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE CASCADE,

    FOREIGN KEY(instrument_id)
        REFERENCES instruments(id)
        ON DELETE RESTRICT
);

CREATE INDEX idx_holdings_account
ON holdings(account_id);

CREATE INDEX idx_holdings_instrument
ON holdings(instrument_id);

CREATE UNIQUE INDEX idx_holdings_active_pair
ON holdings(account_id, instrument_id)
WHERE archived_at IS NULL;

CREATE TABLE account_cash_values (
    id              TEXT PRIMARY KEY NOT NULL,
    account_id      TEXT NOT NULL,

    amount          TEXT NOT NULL,
    currency        TEXT NOT NULL
        CHECK(currency GLOB '[A-Z][A-Z][A-Z]'),

    effective_at    TEXT NOT NULL,
    created_at      TEXT NOT NULL,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE CASCADE
);

CREATE INDEX idx_account_cash_values_latest
ON account_cash_values(
    account_id,
    currency,
    effective_at DESC,
    created_at DESC,
    id DESC
);

CREATE TABLE instrument_quotes (
    id              TEXT PRIMARY KEY NOT NULL,
    instrument_id   TEXT NOT NULL,

    unit_price      TEXT NOT NULL,
    quote_currency  TEXT NOT NULL
        CHECK(quote_currency GLOB '[A-Z][A-Z][A-Z]'),

    source_kind     TEXT NOT NULL
        CHECK(source_kind IN ('manual', 'provider')),
    source_key      TEXT NOT NULL,
    delayed         INTEGER NOT NULL DEFAULT 0
        CHECK(delayed IN (0, 1)),

    quoted_at       TEXT NOT NULL,
    created_at      TEXT NOT NULL,

    FOREIGN KEY(instrument_id)
        REFERENCES instruments(id)
        ON DELETE CASCADE
);

CREATE INDEX idx_instrument_quotes_latest
ON instrument_quotes(
    instrument_id,
    source_kind,
    quoted_at DESC,
    created_at DESC,
    id DESC
);

CREATE TABLE fx_quote_preferences (
    household_id    TEXT NOT NULL,
    currency_a      TEXT NOT NULL
        CHECK(currency_a GLOB '[A-Z][A-Z][A-Z]'),
    currency_b      TEXT NOT NULL
        CHECK(currency_b GLOB '[A-Z][A-Z][A-Z]'),
    source_kind     TEXT NOT NULL
        CHECK(source_kind IN ('manual', 'provider')),
    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,

    PRIMARY KEY(household_id, currency_a, currency_b),
    CHECK(currency_a < currency_b),

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE
);

CREATE TABLE fx_quotes (
    id              TEXT PRIMARY KEY NOT NULL,
    household_id    TEXT NOT NULL,

    base_currency   TEXT NOT NULL
        CHECK(base_currency GLOB '[A-Z][A-Z][A-Z]'),
    quote_currency  TEXT NOT NULL
        CHECK(quote_currency GLOB '[A-Z][A-Z][A-Z]'),
    rate            TEXT NOT NULL,

    source_kind     TEXT NOT NULL
        CHECK(source_kind IN ('manual', 'provider')),
    source_key      TEXT NOT NULL,
    delayed         INTEGER NOT NULL DEFAULT 0
        CHECK(delayed IN (0, 1)),

    quoted_at       TEXT NOT NULL,
    created_at      TEXT NOT NULL,

    CHECK(base_currency != quote_currency),

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE
);

CREATE INDEX idx_fx_quotes_latest
ON fx_quotes(
    household_id,
    base_currency,
    quote_currency,
    source_kind,
    quoted_at DESC,
    created_at DESC,
    id DESC
);
