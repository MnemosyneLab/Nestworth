-- v0.1.6 market-data cache metadata. This migration is schema-only.
-- Provider observations remain append-only quote facts; coverage is mutable,
-- non-financial cache metadata and is written by the application service.

CREATE TABLE market_data_daily_coverage (
    id                  TEXT PRIMARY KEY NOT NULL,
    household_id        TEXT NOT NULL,
    provider_key        TEXT NOT NULL
        CHECK(provider_key IN ('yahoo_finance')),
    target_kind         TEXT NOT NULL
        CHECK(target_kind IN ('instrument', 'fx')),
    instrument_id       TEXT,
    currency_a          TEXT
        CHECK(currency_a IS NULL OR currency_a GLOB '[A-Z][A-Z][A-Z]'),
    currency_b          TEXT
        CHECK(currency_b IS NULL OR currency_b GLOB '[A-Z][A-Z][A-Z]'),
    start_local_date    TEXT NOT NULL
        CHECK(start_local_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    end_local_date      TEXT NOT NULL
        CHECK(end_local_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    updated_at          TEXT NOT NULL,

    CHECK(start_local_date <= end_local_date),
    CHECK(
        (
            target_kind = 'instrument'
            AND instrument_id IS NOT NULL
            AND currency_a IS NULL
            AND currency_b IS NULL
        )
        OR (
            target_kind = 'fx'
            AND instrument_id IS NULL
            AND currency_a IS NOT NULL
            AND currency_b IS NOT NULL
            AND currency_a < currency_b
        )
    ),
    FOREIGN KEY(household_id) REFERENCES households(id) ON DELETE CASCADE,
    FOREIGN KEY(instrument_id) REFERENCES instruments(id) ON DELETE CASCADE
);

CREATE INDEX idx_market_data_daily_coverage_lookup
ON market_data_daily_coverage(
    household_id,
    provider_key,
    target_kind,
    instrument_id,
    currency_a,
    currency_b,
    start_local_date,
    end_local_date
);
