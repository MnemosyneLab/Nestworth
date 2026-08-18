CREATE TABLE history_origins (
    id                      TEXT PRIMARY KEY NOT NULL,
    household_id            TEXT NOT NULL,
    timezone                TEXT NOT NULL,
    timezone_confirmed      INTEGER NOT NULL
        CHECK(timezone_confirmed IN (0, 1)),
    origin_at               TEXT NOT NULL,
    origin_local_date       TEXT NOT NULL,
    source                  TEXT NOT NULL
        CHECK(source IN (
            'migrated_v012',
            'fresh_onboarding'
        )),
    schema_version          INTEGER NOT NULL,
    created_at              TEXT NOT NULL,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE
);

CREATE UNIQUE INDEX idx_history_origins_household
ON history_origins(household_id);

CREATE TABLE history_origin_account_values (
    origin_id       TEXT NOT NULL,
    account_id      TEXT NOT NULL,

    amount          TEXT NOT NULL,
    currency        TEXT NOT NULL
        CHECK(currency GLOB '[A-Z][A-Z][A-Z]'),
    value_kind      TEXT NOT NULL
        CHECK(value_kind IN (
            'balance',
            'manual_value'
        )),

    PRIMARY KEY(origin_id, account_id),

    FOREIGN KEY(origin_id)
        REFERENCES history_origins(id)
        ON DELETE CASCADE,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE RESTRICT
);

CREATE TABLE history_origin_cash_values (
    origin_id       TEXT NOT NULL,
    account_id      TEXT NOT NULL,
    currency        TEXT NOT NULL
        CHECK(currency GLOB '[A-Z][A-Z][A-Z]'),

    amount          TEXT NOT NULL,

    PRIMARY KEY(origin_id, account_id, currency),

    FOREIGN KEY(origin_id)
        REFERENCES history_origins(id)
        ON DELETE CASCADE,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE RESTRICT
);

CREATE TABLE history_origin_holdings (
    origin_id       TEXT NOT NULL,
    holding_id      TEXT NOT NULL,
    account_id      TEXT NOT NULL,
    instrument_id   TEXT NOT NULL,

    quantity        TEXT NOT NULL,
    active          INTEGER NOT NULL
        CHECK(active IN (0, 1)),

    PRIMARY KEY(origin_id, holding_id),

    FOREIGN KEY(origin_id)
        REFERENCES history_origins(id)
        ON DELETE CASCADE,

    FOREIGN KEY(holding_id)
        REFERENCES holdings(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(instrument_id)
        REFERENCES instruments(id)
        ON DELETE RESTRICT
);

CREATE TABLE history_origin_account_states (
    origin_id                   TEXT NOT NULL,
    account_id                  TEXT NOT NULL,

    primary_category            TEXT NOT NULL
        CHECK(primary_category IN (
            'cash_equivalent',
            'investment',
            'property',
            'receivable',
            'liability'
        )),
    secondary_category          TEXT NOT NULL,
    tracking_mode               TEXT NOT NULL
        CHECK(tracking_mode IN (
            'balance',
            'manual_value',
            'holdings'
        )),

    include_in_net_worth        INTEGER NOT NULL
        CHECK(include_in_net_worth IN (0, 1)),
    include_in_investment       INTEGER NOT NULL
        CHECK(include_in_investment IN (0, 1)),
    include_in_liquid_assets    INTEGER NOT NULL
        CHECK(include_in_liquid_assets IN (0, 1)),

    archived_at                 TEXT,
    institution_id              TEXT,
    group_id                    TEXT,

    PRIMARY KEY(origin_id, account_id),

    FOREIGN KEY(origin_id)
        REFERENCES history_origins(id)
        ON DELETE CASCADE,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(institution_id)
        REFERENCES institutions(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(group_id)
        REFERENCES account_groups(id)
        ON DELETE RESTRICT
);

CREATE TABLE history_origin_ownership (
    origin_id       TEXT NOT NULL,
    account_id      TEXT NOT NULL,
    member_id       TEXT NOT NULL,

    share_bps       INTEGER NOT NULL
        CHECK(share_bps > 0 AND share_bps <= 10000),

    PRIMARY KEY(origin_id, account_id, member_id),

    FOREIGN KEY(origin_id)
        REFERENCES history_origins(id)
        ON DELETE CASCADE,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(member_id)
        REFERENCES members(id)
        ON DELETE RESTRICT
);

CREATE TABLE activities (
    id                      TEXT PRIMARY KEY NOT NULL,
    household_id            TEXT NOT NULL,

    kind                    TEXT NOT NULL
        CHECK(kind IN (
            'opening_adjustment',
            'balance_adjustment',
            'position_adjustment',
            'deposit',
            'withdrawal',
            'transfer',
            'buy',
            'sell',
            'income',
            'fee',
            'debt_draw',
            'debt_payment',
            'debt_adjustment',
            'manual_valuation',
            'reversal'
        )),

    effective_at            TEXT NOT NULL,
    effective_local_date    TEXT NOT NULL,
    created_at              TEXT NOT NULL,
    note                    TEXT,

    reverses                TEXT,
    corrects                TEXT,
    correction_group        TEXT,

    income_kind             TEXT
        CHECK(income_kind IS NULL OR income_kind IN (
            'salary',
            'bonus',
            'dividend',
            'interest',
            'rental',
            'pension',
            'gift',
            'refund',
            'other'
        )),
    fee_kind                TEXT
        CHECK(fee_kind IS NULL OR fee_kind IN (
            'bank_fee',
            'account_fee',
            'brokerage_commission',
            'management_fee',
            'foreign_exchange_fee',
            'interest',
            'tax',
            'other'
        )),
    related_instrument_id   TEXT,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE,

    FOREIGN KEY(reverses)
        REFERENCES activities(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(corrects)
        REFERENCES activities(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(related_instrument_id)
        REFERENCES instruments(id)
        ON DELETE RESTRICT
);

CREATE INDEX idx_activities_household_cursor
ON activities(
    household_id,
    effective_at DESC,
    created_at DESC,
    id DESC
);

CREATE UNIQUE INDEX idx_activities_reverses
ON activities(reverses)
WHERE reverses IS NOT NULL;

CREATE INDEX idx_activities_corrects
ON activities(corrects)
WHERE corrects IS NOT NULL;

CREATE INDEX idx_activities_correction_group
ON activities(correction_group)
WHERE correction_group IS NOT NULL;

CREATE TABLE activity_legs (
    id              TEXT PRIMARY KEY NOT NULL,
    activity_id     TEXT NOT NULL,
    account_id      TEXT NOT NULL,

    role            TEXT NOT NULL
        CHECK(role IN (
            'source',
            'destination',
            'holding',
            'settlement',
            'fee',
            'income',
            'liability',
            'adjustment'
        )),
    direction       TEXT NOT NULL
        CHECK(direction IN (
            'increase',
            'decrease'
        )),
    component_kind  TEXT NOT NULL
        CHECK(component_kind IN (
            'account_value',
            'holdings_cash',
            'holding_quantity'
        )),

    amount          TEXT,
    currency        TEXT
        CHECK(currency IS NULL OR currency GLOB '[A-Z][A-Z][A-Z]'),
    holding_id      TEXT,
    instrument_id   TEXT,
    quantity        TEXT,
    fx_rate         TEXT,
    sort_order      INTEGER NOT NULL,

    FOREIGN KEY(activity_id)
        REFERENCES activities(id)
        ON DELETE CASCADE,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(holding_id)
        REFERENCES holdings(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(instrument_id)
        REFERENCES instruments(id)
        ON DELETE RESTRICT,

    CHECK(
        (
            component_kind IN ('account_value', 'holdings_cash')
            AND amount IS NOT NULL
            AND currency IS NOT NULL
            AND holding_id IS NULL
            AND instrument_id IS NULL
            AND quantity IS NULL
        )
        OR (
            component_kind = 'holding_quantity'
            AND quantity IS NOT NULL
            AND holding_id IS NOT NULL
            AND instrument_id IS NOT NULL
            AND amount IS NULL
            AND currency IS NULL
            AND fx_rate IS NULL
        )
    )
);

CREATE INDEX idx_activity_legs_activity
ON activity_legs(activity_id, sort_order, id);

CREATE INDEX idx_activity_legs_account
ON activity_legs(account_id);

CREATE INDEX idx_activity_legs_instrument
ON activity_legs(instrument_id)
WHERE instrument_id IS NOT NULL;

ALTER TABLE account_values
ADD COLUMN activity_id TEXT REFERENCES activities(id) ON DELETE RESTRICT;

ALTER TABLE account_cash_values
ADD COLUMN activity_id TEXT REFERENCES activities(id) ON DELETE RESTRICT;

CREATE INDEX idx_account_values_activity
ON account_values(activity_id)
WHERE activity_id IS NOT NULL;

CREATE INDEX idx_account_cash_values_activity
ON account_cash_values(activity_id)
WHERE activity_id IS NOT NULL;

CREATE TABLE holding_quantity_values (
    id              TEXT PRIMARY KEY NOT NULL,
    holding_id      TEXT NOT NULL,

    quantity        TEXT NOT NULL,
    effective_at    TEXT NOT NULL,
    created_at      TEXT NOT NULL,
    activity_id     TEXT,

    FOREIGN KEY(holding_id)
        REFERENCES holdings(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(activity_id)
        REFERENCES activities(id)
        ON DELETE RESTRICT
);

CREATE INDEX idx_holding_quantity_values_latest
ON holding_quantity_values(
    holding_id,
    effective_at DESC,
    created_at DESC,
    id DESC
);

CREATE INDEX idx_holding_quantity_values_activity
ON holding_quantity_values(activity_id)
WHERE activity_id IS NOT NULL;

CREATE TABLE account_state_observations (
    id                          TEXT PRIMARY KEY NOT NULL,
    account_id                  TEXT NOT NULL,

    primary_category            TEXT NOT NULL
        CHECK(primary_category IN (
            'cash_equivalent',
            'investment',
            'property',
            'receivable',
            'liability'
        )),
    secondary_category          TEXT NOT NULL,
    tracking_mode               TEXT NOT NULL
        CHECK(tracking_mode IN (
            'balance',
            'manual_value',
            'holdings'
        )),

    include_in_net_worth        INTEGER NOT NULL
        CHECK(include_in_net_worth IN (0, 1)),
    include_in_investment       INTEGER NOT NULL
        CHECK(include_in_investment IN (0, 1)),
    include_in_liquid_assets    INTEGER NOT NULL
        CHECK(include_in_liquid_assets IN (0, 1)),

    archived_at                 TEXT,
    institution_id              TEXT,
    group_id                    TEXT,

    effective_at                TEXT NOT NULL,
    created_at                  TEXT NOT NULL,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(institution_id)
        REFERENCES institutions(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(group_id)
        REFERENCES account_groups(id)
        ON DELETE RESTRICT
);

CREATE INDEX idx_account_state_observations_latest
ON account_state_observations(
    account_id,
    effective_at DESC,
    created_at DESC,
    id DESC
);

CREATE TABLE account_state_ownership (
    observation_id  TEXT NOT NULL,
    member_id       TEXT NOT NULL,

    share_bps       INTEGER NOT NULL
        CHECK(share_bps > 0 AND share_bps <= 10000),

    PRIMARY KEY(observation_id, member_id),

    FOREIGN KEY(observation_id)
        REFERENCES account_state_observations(id)
        ON DELETE CASCADE,

    FOREIGN KEY(member_id)
        REFERENCES members(id)
        ON DELETE RESTRICT
);

CREATE INDEX idx_account_state_ownership_member
ON account_state_ownership(member_id);

CREATE TABLE holding_state_observations (
    id              TEXT PRIMARY KEY NOT NULL,
    holding_id      TEXT NOT NULL,

    active          INTEGER NOT NULL
        CHECK(active IN (0, 1)),
    archived_at     TEXT,

    effective_at    TEXT NOT NULL,
    created_at      TEXT NOT NULL,

    FOREIGN KEY(holding_id)
        REFERENCES holdings(id)
        ON DELETE RESTRICT
);

CREATE INDEX idx_holding_state_observations_latest
ON holding_state_observations(
    holding_id,
    effective_at DESC,
    created_at DESC,
    id DESC
);

CREATE TABLE instrument_preference_observations (
    id                  TEXT PRIMARY KEY NOT NULL,
    instrument_id       TEXT NOT NULL,

    quote_preference    TEXT NOT NULL
        CHECK(quote_preference IN ('manual', 'provider')),

    effective_at        TEXT NOT NULL,
    created_at          TEXT NOT NULL,

    FOREIGN KEY(instrument_id)
        REFERENCES instruments(id)
        ON DELETE RESTRICT
);

CREATE INDEX idx_instrument_preference_observations_latest
ON instrument_preference_observations(
    instrument_id,
    effective_at DESC,
    created_at DESC,
    id DESC
);

CREATE TABLE fx_preference_observations (
    id              TEXT PRIMARY KEY NOT NULL,
    household_id    TEXT NOT NULL,
    currency_a      TEXT NOT NULL
        CHECK(currency_a GLOB '[A-Z][A-Z][A-Z]'),
    currency_b      TEXT NOT NULL
        CHECK(currency_b GLOB '[A-Z][A-Z][A-Z]'),
    source_kind     TEXT NOT NULL
        CHECK(source_kind IN ('manual', 'provider')),

    effective_at    TEXT NOT NULL,
    created_at      TEXT NOT NULL,

    CHECK(currency_a < currency_b),

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE
);

CREATE INDEX idx_fx_preference_observations_latest
ON fx_preference_observations(
    household_id,
    currency_a,
    currency_b,
    effective_at DESC,
    created_at DESC,
    id DESC
);

CREATE TABLE daily_valuation_snapshots (
    id                          TEXT PRIMARY KEY NOT NULL,
    household_id                TEXT NOT NULL,

    snapshot_on                 TEXT NOT NULL,
    cutoff_at                   TEXT NOT NULL,
    revision                    INTEGER NOT NULL,
    supersedes_snapshot_id      TEXT,

    assets_amount               TEXT NOT NULL,
    liabilities_amount          TEXT NOT NULL,
    net_worth_amount            TEXT NOT NULL,
    currency                    TEXT NOT NULL
        CHECK(currency GLOB '[A-Z][A-Z][A-Z]'),

    is_complete                 INTEGER NOT NULL
        CHECK(is_complete IN (0, 1)),
    valued_component_count      INTEGER NOT NULL,
    total_component_count       INTEGER NOT NULL,
    coverage_bps                INTEGER NOT NULL,

    generation_reason           TEXT NOT NULL,
    created_at                  TEXT NOT NULL,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE,

    FOREIGN KEY(supersedes_snapshot_id)
        REFERENCES daily_valuation_snapshots(id)
        ON DELETE RESTRICT
);

CREATE UNIQUE INDEX idx_daily_valuation_snapshots_revision
ON daily_valuation_snapshots(household_id, snapshot_on, revision);

CREATE INDEX idx_daily_valuation_snapshots_latest
ON daily_valuation_snapshots(
    household_id,
    snapshot_on DESC,
    revision DESC,
    created_at DESC,
    id DESC
);

CREATE TABLE daily_valuation_snapshot_items (
    id                              TEXT PRIMARY KEY NOT NULL,
    snapshot_id                     TEXT NOT NULL,

    account_id                      TEXT NOT NULL,
    holding_id                      TEXT,
    instrument_id                   TEXT,

    component_kind                  TEXT NOT NULL
        CHECK(component_kind IN (
            'account_value',
            'holdings_cash',
            'holding_quantity'
        )),

    native_amount                   TEXT,
    native_currency                 TEXT
        CHECK(native_currency IS NULL OR native_currency GLOB '[A-Z][A-Z][A-Z]'),
    base_amount                     TEXT,

    instrument_quote_id             TEXT,
    fx_quote_id                     TEXT,
    account_state_observation_id    TEXT,
    origin_id                       TEXT,
    activity_id                     TEXT,

    is_complete                     INTEGER NOT NULL
        CHECK(is_complete IN (0, 1)),
    missing_reason                  TEXT,
    sort_order                      INTEGER NOT NULL DEFAULT 0,

    FOREIGN KEY(snapshot_id)
        REFERENCES daily_valuation_snapshots(id)
        ON DELETE CASCADE,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(holding_id)
        REFERENCES holdings(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(instrument_id)
        REFERENCES instruments(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(instrument_quote_id)
        REFERENCES instrument_quotes(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(fx_quote_id)
        REFERENCES fx_quotes(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(account_state_observation_id)
        REFERENCES account_state_observations(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(origin_id)
        REFERENCES history_origins(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(activity_id)
        REFERENCES activities(id)
        ON DELETE RESTRICT
);

CREATE INDEX idx_daily_valuation_snapshot_items_account
ON daily_valuation_snapshot_items(snapshot_id, account_id, sort_order, id);

CREATE INDEX idx_daily_valuation_snapshot_items_instrument
ON daily_valuation_snapshot_items(snapshot_id, instrument_id)
WHERE instrument_id IS NOT NULL;

CREATE TABLE history_snapshot_state (
    household_id        TEXT PRIMARY KEY NOT NULL,

    dirty_from          TEXT,
    last_completed_on   TEXT,
    rebuild_status      TEXT NOT NULL DEFAULT 'idle'
        CHECK(rebuild_status IN (
            'idle',
            'running',
            'cancelled',
            'failed'
        )),
    rebuild_cursor_on   TEXT,
    updated_at          TEXT NOT NULL,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE
);
