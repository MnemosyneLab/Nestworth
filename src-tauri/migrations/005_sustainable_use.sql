-- v0.1.5 sustainable-use schema.
-- This migration is deliberately schema-only.  Default policy rows are created
-- by the post-migration initializer after SQLite integrity has been checked.

CREATE TABLE recurring_activity_rules (
    id                      TEXT PRIMARY KEY NOT NULL,
    household_id            TEXT NOT NULL,

    cadence                 TEXT NOT NULL
        CHECK(cadence IN ('daily', 'weekly', 'monthly', 'yearly')),
    interval_value          INTEGER NOT NULL
        CHECK(
            (cadence = 'daily' AND interval_value BETWEEN 1 AND 365)
            OR (cadence = 'weekly' AND interval_value BETWEEN 1 AND 52)
            OR (cadence = 'monthly' AND interval_value BETWEEN 1 AND 24)
            OR (cadence = 'yearly' AND interval_value BETWEEN 1 AND 10)
        ),
    start_local_date        TEXT NOT NULL
        CHECK(start_local_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    end_local_date          TEXT
        CHECK(end_local_date IS NULL OR end_local_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    anchor_local_date       TEXT NOT NULL
        CHECK(anchor_local_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),

    kind                    TEXT NOT NULL
        CHECK(kind IN (
            'deposit', 'withdrawal', 'transfer', 'income', 'fee',
            'debt_draw', 'debt_payment'
        )),

    endpoint_account_id     TEXT,
    endpoint_component      TEXT
        CHECK(endpoint_component IS NULL OR endpoint_component IN ('account_value', 'holdings_cash')),
    amount                  TEXT,
    currency                TEXT
        CHECK(currency IS NULL OR currency GLOB '[A-Z][A-Z][A-Z]'),

    source_account_id       TEXT,
    source_component        TEXT
        CHECK(source_component IS NULL OR source_component IN ('account_value', 'holdings_cash')),
    source_amount           TEXT,
    source_currency         TEXT
        CHECK(source_currency IS NULL OR source_currency GLOB '[A-Z][A-Z][A-Z]'),
    destination_account_id  TEXT,
    destination_component   TEXT
        CHECK(destination_component IS NULL OR destination_component IN ('account_value', 'holdings_cash')),
    destination_amount      TEXT,
    destination_currency    TEXT
        CHECK(destination_currency IS NULL OR destination_currency GLOB '[A-Z][A-Z][A-Z]'),
    fee_amount              TEXT,
    fee_currency            TEXT
        CHECK(fee_currency IS NULL OR fee_currency GLOB '[A-Z][A-Z][A-Z]'),
    fee_kind                TEXT
        CHECK(fee_kind IS NULL OR fee_kind IN (
            'bank_fee', 'account_fee', 'brokerage_commission', 'management_fee',
            'foreign_exchange_fee', 'interest', 'tax', 'other'
        )),
    income_kind             TEXT
        CHECK(income_kind IS NULL OR income_kind IN (
            'salary', 'bonus', 'dividend', 'interest', 'rental',
            'pension', 'gift', 'refund', 'other'
        )),
    related_instrument_id   TEXT,
    liability_account_id    TEXT,
    principal_amount        TEXT,
    principal_currency      TEXT
        CHECK(principal_currency IS NULL OR principal_currency GLOB '[A-Z][A-Z][A-Z]'),
    cash_account_id         TEXT,
    cash_component          TEXT
        CHECK(cash_component IS NULL OR cash_component IN ('account_value', 'holdings_cash')),
    cash_amount             TEXT,
    cash_currency           TEXT
        CHECK(cash_currency IS NULL OR cash_currency GLOB '[A-Z][A-Z][A-Z]'),
    fx_rate                 TEXT,

    note                    TEXT,
    revision                INTEGER NOT NULL DEFAULT 1 CHECK(revision > 0),
    archived_at             TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,

    CHECK(end_local_date IS NULL OR end_local_date >= start_local_date),
    CHECK(anchor_local_date <= start_local_date),
    CHECK(
        (
            kind IN ('deposit', 'withdrawal', 'income', 'fee')
            AND endpoint_account_id IS NOT NULL
            AND endpoint_component IS NOT NULL
            AND amount IS NOT NULL
            AND currency IS NOT NULL
        )
        OR (
            kind = 'transfer'
            AND source_account_id IS NOT NULL
            AND source_component IS NOT NULL
            AND source_amount IS NOT NULL
            AND source_currency IS NOT NULL
            AND destination_account_id IS NOT NULL
            AND destination_component IS NOT NULL
            AND destination_amount IS NOT NULL
            AND destination_currency IS NOT NULL
            AND source_currency = destination_currency
            AND (source_account_id <> destination_account_id OR source_component <> destination_component)
        )
        OR (
            kind = 'debt_draw'
            AND liability_account_id IS NOT NULL
            AND principal_amount IS NOT NULL
            AND principal_currency IS NOT NULL
            AND (cash_account_id IS NULL) = (cash_amount IS NULL)
            AND (cash_account_id IS NULL) = (cash_currency IS NULL)
            AND (cash_account_id IS NULL OR cash_component IS NOT NULL)
        )
        OR (
            kind = 'debt_payment'
            AND liability_account_id IS NOT NULL
            AND principal_amount IS NOT NULL
            AND principal_currency IS NOT NULL
            AND cash_account_id IS NOT NULL
            AND cash_component IS NOT NULL
            AND cash_amount IS NOT NULL
            AND cash_currency IS NOT NULL
        )
    ),
    CHECK(
        (kind = 'income' AND income_kind IS NOT NULL)
        OR (kind <> 'income' AND income_kind IS NULL)
    ),
    CHECK(
        (kind = 'fee' AND fee_kind IS NOT NULL)
        OR (kind NOT IN ('fee', 'transfer', 'debt_payment') AND fee_kind IS NULL)
    ),
    CHECK(
        (kind = 'transfer' AND (fee_amount IS NULL) = (fee_currency IS NULL)
            AND (fee_amount IS NULL) = (fee_kind IS NULL))
        OR (kind = 'debt_payment' AND (fee_amount IS NULL) = (fee_currency IS NULL)
            AND (fee_amount IS NULL) = (fee_kind IS NULL))
        OR (kind NOT IN ('transfer', 'debt_payment') AND fee_amount IS NULL AND fee_currency IS NULL AND fee_kind IS NULL)
    ),
    FOREIGN KEY(household_id) REFERENCES households(id) ON DELETE CASCADE,
    FOREIGN KEY(endpoint_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(source_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(destination_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(liability_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(cash_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(related_instrument_id) REFERENCES instruments(id) ON DELETE RESTRICT
);

CREATE INDEX idx_recurring_activity_rules_due
ON recurring_activity_rules(household_id, archived_at, start_local_date, id);

CREATE INDEX idx_recurring_activity_rules_updated
ON recurring_activity_rules(household_id, updated_at, id);

CREATE TABLE pending_activities (
    id                      TEXT PRIMARY KEY NOT NULL,
    household_id            TEXT NOT NULL,
    recurring_rule_id       TEXT,
    recurring_rule_revision INTEGER,
    scheduled_local_date    TEXT NOT NULL
        CHECK(scheduled_local_date GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    creation_source         TEXT NOT NULL CHECK(creation_source IN ('manual', 'recurring')),
    kind                    TEXT NOT NULL
        CHECK(kind IN (
            'deposit', 'withdrawal', 'transfer', 'position_transfer',
            'buy', 'sell', 'income', 'fee', 'debt_draw', 'debt_payment'
        )),

    endpoint_account_id     TEXT,
    endpoint_component      TEXT
        CHECK(endpoint_component IS NULL OR endpoint_component IN ('account_value', 'holdings_cash')),
    amount                  TEXT,
    currency                TEXT
        CHECK(currency IS NULL OR currency GLOB '[A-Z][A-Z][A-Z]'),
    source_account_id       TEXT,
    source_component        TEXT
        CHECK(source_component IS NULL OR source_component IN ('account_value', 'holdings_cash')),
    source_amount           TEXT,
    source_currency         TEXT
        CHECK(source_currency IS NULL OR source_currency GLOB '[A-Z][A-Z][A-Z]'),
    destination_account_id  TEXT,
    destination_component   TEXT
        CHECK(destination_component IS NULL OR destination_component IN ('account_value', 'holdings_cash')),
    destination_amount      TEXT,
    destination_currency    TEXT
        CHECK(destination_currency IS NULL OR destination_currency GLOB '[A-Z][A-Z][A-Z]'),
    fee_amount              TEXT,
    fee_currency            TEXT
        CHECK(fee_currency IS NULL OR fee_currency GLOB '[A-Z][A-Z][A-Z]'),
    fee_kind                TEXT
        CHECK(fee_kind IS NULL OR fee_kind IN (
            'bank_fee', 'account_fee', 'brokerage_commission', 'management_fee',
            'foreign_exchange_fee', 'interest', 'tax', 'other'
        )),
    income_kind             TEXT
        CHECK(income_kind IS NULL OR income_kind IN (
            'salary', 'bonus', 'dividend', 'interest', 'rental',
            'pension', 'gift', 'refund', 'other'
        )),
    related_instrument_id   TEXT,
    source_holding_id       TEXT,
    source_instrument_id    TEXT,
    destination_holding_id  TEXT,
    destination_instrument_id TEXT,
    quantity                TEXT,
    holding_id              TEXT,
    instrument_id           TEXT,
    unit_price              TEXT,
    gross_amount            TEXT,
    gross_currency          TEXT
        CHECK(gross_currency IS NULL OR gross_currency GLOB '[A-Z][A-Z][A-Z]'),
    confirm_zero_unit_price INTEGER NOT NULL DEFAULT 0 CHECK(confirm_zero_unit_price IN (0, 1)),
    liability_account_id    TEXT,
    principal_amount        TEXT,
    principal_currency      TEXT
        CHECK(principal_currency IS NULL OR principal_currency GLOB '[A-Z][A-Z][A-Z]'),
    cash_account_id         TEXT,
    cash_component          TEXT
        CHECK(cash_component IS NULL OR cash_component IN ('account_value', 'holdings_cash')),
    cash_amount             TEXT,
    cash_currency           TEXT
        CHECK(cash_currency IS NULL OR cash_currency GLOB '[A-Z][A-Z][A-Z]'),
    fx_rate                 TEXT,

    note                    TEXT,
    status                  TEXT NOT NULL CHECK(status IN ('open', 'posted', 'skipped')),
    posted_activity_id      TEXT,
    skipped_at              TEXT,
    created_at              TEXT NOT NULL,
    updated_at              TEXT NOT NULL,

    CHECK((recurring_rule_id IS NULL) = (recurring_rule_revision IS NULL)),
    CHECK(creation_source = 'manual' OR recurring_rule_id IS NOT NULL),
    CHECK(
        (status = 'open' AND posted_activity_id IS NULL AND skipped_at IS NULL)
        OR (status = 'posted' AND posted_activity_id IS NOT NULL AND skipped_at IS NULL)
        OR (status = 'skipped' AND posted_activity_id IS NULL AND skipped_at IS NOT NULL)
    ),
    CHECK(
        (
            kind IN ('deposit', 'withdrawal', 'income', 'fee')
            AND endpoint_account_id IS NOT NULL
            AND endpoint_component IS NOT NULL
            AND amount IS NOT NULL
            AND currency IS NOT NULL
        )
        OR (
            kind = 'transfer'
            AND source_account_id IS NOT NULL
            AND source_component IS NOT NULL
            AND source_amount IS NOT NULL
            AND source_currency IS NOT NULL
            AND destination_account_id IS NOT NULL
            AND destination_component IS NOT NULL
            AND destination_amount IS NOT NULL
            AND destination_currency IS NOT NULL
        )
        OR (
            kind = 'position_transfer'
            AND source_account_id IS NOT NULL
            AND source_holding_id IS NOT NULL
            AND source_instrument_id IS NOT NULL
            AND destination_account_id IS NOT NULL
            AND destination_holding_id IS NOT NULL
            AND destination_instrument_id IS NOT NULL
            AND source_instrument_id = destination_instrument_id
            AND quantity IS NOT NULL
        )
        OR (
            kind IN ('buy', 'sell')
            AND holding_id IS NOT NULL
            AND instrument_id IS NOT NULL
            AND quantity IS NOT NULL
            AND unit_price IS NOT NULL
            AND gross_amount IS NOT NULL
            AND gross_currency IS NOT NULL
        )
        OR (
            kind = 'debt_draw'
            AND liability_account_id IS NOT NULL
            AND principal_amount IS NOT NULL
            AND principal_currency IS NOT NULL
        )
        OR (
            kind = 'debt_payment'
            AND liability_account_id IS NOT NULL
            AND principal_amount IS NOT NULL
            AND principal_currency IS NOT NULL
            AND cash_account_id IS NOT NULL
            AND cash_component IS NOT NULL
            AND cash_amount IS NOT NULL
            AND cash_currency IS NOT NULL
        )
    ),
    CHECK(
        (kind = 'income' AND income_kind IS NOT NULL)
        OR (kind <> 'income' AND income_kind IS NULL)
    ),
    CHECK(
        (kind = 'fee' AND fee_kind IS NOT NULL)
        OR (kind NOT IN ('fee', 'transfer', 'debt_payment') AND fee_kind IS NULL)
    ),
    FOREIGN KEY(household_id) REFERENCES households(id) ON DELETE CASCADE,
    FOREIGN KEY(recurring_rule_id) REFERENCES recurring_activity_rules(id) ON DELETE RESTRICT,
    FOREIGN KEY(endpoint_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(source_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(destination_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(liability_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(cash_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(related_instrument_id) REFERENCES instruments(id) ON DELETE RESTRICT,
    FOREIGN KEY(source_holding_id) REFERENCES holdings(id) ON DELETE RESTRICT,
    FOREIGN KEY(source_instrument_id) REFERENCES instruments(id) ON DELETE RESTRICT,
    FOREIGN KEY(destination_holding_id) REFERENCES holdings(id) ON DELETE RESTRICT,
    FOREIGN KEY(destination_instrument_id) REFERENCES instruments(id) ON DELETE RESTRICT,
    FOREIGN KEY(holding_id) REFERENCES holdings(id) ON DELETE RESTRICT,
    FOREIGN KEY(instrument_id) REFERENCES instruments(id) ON DELETE RESTRICT,
    FOREIGN KEY(posted_activity_id) REFERENCES activities(id) ON DELETE RESTRICT,
    UNIQUE(recurring_rule_id, scheduled_local_date)
);

CREATE INDEX idx_pending_activities_due
ON pending_activities(household_id, status, scheduled_local_date, id);

CREATE INDEX idx_pending_activities_rule
ON pending_activities(recurring_rule_id, scheduled_local_date, id);

CREATE INDEX idx_pending_activities_posted_activity
ON pending_activities(posted_activity_id);

CREATE TABLE freshness_policies (
    id                  TEXT PRIMARY KEY NOT NULL,
    household_id        TEXT NOT NULL,
    kind                TEXT NOT NULL CHECK(kind IN (
        'account_value', 'account_cash', 'instrument_quote', 'fx_quote'
    )),
    target_account_id   TEXT,
    target_instrument_id TEXT,
    target_currency_a   TEXT,
    target_currency_b   TEXT,
    review_interval_days INTEGER
        CHECK(review_interval_days IS NULL OR review_interval_days BETWEEN 1 AND 3650),
    archived_at         TEXT,
    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,

    CHECK((target_currency_a IS NULL) = (target_currency_b IS NULL)),
    CHECK(
        (target_account_id IS NOT NULL)
        + (target_instrument_id IS NOT NULL)
        + (target_currency_a IS NOT NULL) <= 1
    ),
    CHECK(
        (
            target_account_id IS NULL
            AND target_instrument_id IS NULL
            AND target_currency_a IS NULL
            AND target_currency_b IS NULL
        )
        OR (
            kind IN ('account_value', 'account_cash')
            AND target_account_id IS NOT NULL
            AND target_instrument_id IS NULL
            AND target_currency_a IS NULL
            AND target_currency_b IS NULL
        )
        OR (
            kind = 'instrument_quote'
            AND target_account_id IS NULL
            AND target_instrument_id IS NOT NULL
            AND target_currency_a IS NULL
            AND target_currency_b IS NULL
        )
        OR (
            kind = 'fx_quote'
            AND target_account_id IS NULL
            AND target_instrument_id IS NULL
            AND target_currency_a IS NOT NULL
            AND target_currency_b IS NOT NULL
            AND target_currency_a < target_currency_b
        )
    ),
    CHECK(target_currency_a IS NULL OR target_currency_a GLOB '[A-Z][A-Z][A-Z]'),
    CHECK(target_currency_b IS NULL OR target_currency_b GLOB '[A-Z][A-Z][A-Z]'),
    FOREIGN KEY(household_id) REFERENCES households(id) ON DELETE CASCADE,
    FOREIGN KEY(target_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(target_instrument_id) REFERENCES instruments(id) ON DELETE RESTRICT
);

CREATE UNIQUE INDEX uq_freshness_policies_default
ON freshness_policies(household_id, kind)
WHERE target_account_id IS NULL AND target_instrument_id IS NULL AND target_currency_a IS NULL;

CREATE UNIQUE INDEX uq_freshness_policies_account_target
ON freshness_policies(household_id, kind, target_account_id)
WHERE target_account_id IS NOT NULL;

CREATE UNIQUE INDEX uq_freshness_policies_instrument_target
ON freshness_policies(household_id, kind, target_instrument_id)
WHERE target_instrument_id IS NOT NULL;

CREATE UNIQUE INDEX uq_freshness_policies_fx_target
ON freshness_policies(household_id, kind, target_currency_a, target_currency_b)
WHERE target_currency_a IS NOT NULL;

CREATE INDEX idx_freshness_policies_resolution
ON freshness_policies(household_id, kind, archived_at, target_account_id, target_instrument_id, target_currency_a, target_currency_b);

CREATE TABLE maintenance_snoozes (
    id                  TEXT PRIMARY KEY NOT NULL,
    household_id        TEXT NOT NULL,
    policy_kind         TEXT NOT NULL CHECK(policy_kind IN (
        'account_value', 'account_cash', 'instrument_quote', 'fx_quote'
    )),
    target_account_id   TEXT,
    target_instrument_id TEXT,
    target_currency_a   TEXT,
    target_currency_b   TEXT,
    snoozed_until       TEXT NOT NULL
        CHECK(snoozed_until GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    created_at          TEXT NOT NULL,

    CHECK((target_currency_a IS NULL) = (target_currency_b IS NULL)),
    CHECK(
        (target_account_id IS NOT NULL)
        + (target_instrument_id IS NOT NULL)
        + (target_currency_a IS NOT NULL) = 1
    ),
    CHECK(
        (policy_kind IN ('account_value', 'account_cash') AND target_account_id IS NOT NULL AND target_instrument_id IS NULL AND target_currency_a IS NULL AND target_currency_b IS NULL)
        OR (policy_kind = 'instrument_quote' AND target_account_id IS NULL AND target_instrument_id IS NOT NULL AND target_currency_a IS NULL AND target_currency_b IS NULL)
        OR (policy_kind = 'fx_quote' AND target_account_id IS NULL AND target_instrument_id IS NULL AND target_currency_a IS NOT NULL AND target_currency_b IS NOT NULL AND target_currency_a < target_currency_b)
    ),
    CHECK(target_currency_a IS NULL OR target_currency_a GLOB '[A-Z][A-Z][A-Z]'),
    CHECK(target_currency_b IS NULL OR target_currency_b GLOB '[A-Z][A-Z][A-Z]'),
    FOREIGN KEY(household_id) REFERENCES households(id) ON DELETE CASCADE,
    FOREIGN KEY(target_account_id) REFERENCES accounts(id) ON DELETE RESTRICT,
    FOREIGN KEY(target_instrument_id) REFERENCES instruments(id) ON DELETE RESTRICT
);

CREATE INDEX idx_maintenance_snoozes_lookup
ON maintenance_snoozes(household_id, policy_kind, snoozed_until, target_account_id, target_instrument_id, target_currency_a, target_currency_b, created_at, id);

CREATE TABLE import_batches (
    id                  TEXT PRIMARY KEY NOT NULL,
    household_id        TEXT NOT NULL,
    template             TEXT NOT NULL CHECK(template IN (
        '# nestworth-csv:activity:v1',
        '# nestworth-csv:quote:v1',
        '# nestworth-csv:benchmark:v1'
    )),
    file_sha256          TEXT NOT NULL
        CHECK(length(file_sha256) = 64 AND file_sha256 NOT GLOB '*[^0-9a-f]*'),
    source_namespace     TEXT,
    row_count            INTEGER NOT NULL DEFAULT 0 CHECK(row_count >= 0),
    committed_count      INTEGER NOT NULL DEFAULT 0 CHECK(committed_count >= 0),
    duplicate_count      INTEGER NOT NULL DEFAULT 0 CHECK(duplicate_count >= 0),
    rejected_count       INTEGER NOT NULL DEFAULT 0 CHECK(rejected_count >= 0),
    status               TEXT NOT NULL CHECK(status IN ('previewed', 'committed', 'failed', 'cancelled')),
    created_at           TEXT NOT NULL,
    completed_at         TEXT,
    CHECK(source_namespace IS NULL OR (
        length(source_namespace) BETWEEN 1 AND 80
        AND source_namespace NOT GLOB '*[^a-z0-9._-]*'
        AND substr(source_namespace, 1, 1) NOT GLOB '[^a-z0-9]'
    )),
    CHECK(committed_count + duplicate_count + rejected_count <= row_count),
    CHECK((status = 'previewed' AND completed_at IS NULL) OR (status <> 'previewed')),
    FOREIGN KEY(household_id) REFERENCES households(id) ON DELETE CASCADE
);

CREATE INDEX idx_import_batches_household
ON import_batches(household_id, created_at DESC, id DESC);

CREATE TABLE import_items (
    id                      TEXT PRIMARY KEY NOT NULL,
    batch_id                TEXT NOT NULL,
    row_number              INTEGER NOT NULL CHECK(row_number > 0),
    source_namespace        TEXT,
    external_id             TEXT,
    fingerprint             TEXT NOT NULL
        CHECK(length(fingerprint) = 64 AND fingerprint NOT GLOB '*[^0-9a-f]*'),
    outcome                 TEXT NOT NULL CHECK(outcome IN ('committed', 'exact_duplicate', 'rejected', 'conflict')),
    diagnostic_code         TEXT,
    activity_id             TEXT,
    instrument_quote_id     TEXT,
    fx_quote_id             TEXT,
    benchmark_observation_id TEXT,
    created_at              TEXT NOT NULL,

    CHECK((source_namespace IS NULL) = (external_id IS NULL)),
    CHECK(
        (
            outcome IN ('committed', 'exact_duplicate')
            AND (activity_id IS NOT NULL)
                + (instrument_quote_id IS NOT NULL)
                + (fx_quote_id IS NOT NULL)
                + (benchmark_observation_id IS NOT NULL) = 1
        )
        OR (
            outcome IN ('rejected', 'conflict')
            AND activity_id IS NULL
            AND instrument_quote_id IS NULL
            AND fx_quote_id IS NULL
            AND benchmark_observation_id IS NULL
        )
    ),
    FOREIGN KEY(batch_id) REFERENCES import_batches(id) ON DELETE RESTRICT,
    FOREIGN KEY(activity_id) REFERENCES activities(id) ON DELETE RESTRICT,
    FOREIGN KEY(instrument_quote_id) REFERENCES instrument_quotes(id) ON DELETE RESTRICT,
    FOREIGN KEY(fx_quote_id) REFERENCES fx_quotes(id) ON DELETE RESTRICT,
    FOREIGN KEY(benchmark_observation_id) REFERENCES benchmark_observations(id) ON DELETE RESTRICT
);

CREATE INDEX idx_import_items_batch_row
ON import_items(batch_id, row_number, id);

CREATE INDEX idx_import_items_identity
ON import_items(source_namespace, external_id, fingerprint, created_at, id);

CREATE INDEX idx_import_items_target
ON import_items(activity_id, instrument_quote_id, fx_quote_id, benchmark_observation_id);

CREATE TABLE benchmarks (
    id                  TEXT PRIMARY KEY NOT NULL,
    household_id        TEXT NOT NULL,
    name                TEXT NOT NULL,
    currency             TEXT NOT NULL CHECK(currency GLOB '[A-Z][A-Z][A-Z]'),
    series_kind          TEXT NOT NULL CHECK(series_kind IN ('price_return', 'total_return')),
    max_carry_days       INTEGER NOT NULL DEFAULT 7 CHECK(max_carry_days BETWEEN 0 AND 31),
    archived_at          TEXT,
    created_at           TEXT NOT NULL,
    updated_at           TEXT NOT NULL,
    FOREIGN KEY(household_id) REFERENCES households(id) ON DELETE CASCADE
);

CREATE INDEX idx_benchmarks_household
ON benchmarks(household_id, archived_at, name COLLATE NOCASE, id);

CREATE TABLE benchmark_observations (
    id                  TEXT PRIMARY KEY NOT NULL,
    benchmark_id        TEXT NOT NULL,
    level               TEXT NOT NULL CHECK(length(level) > 0),
    observed_on         TEXT NOT NULL
        CHECK(observed_on GLOB '[0-9][0-9][0-9][0-9]-[0-9][0-9]-[0-9][0-9]'),
    note                TEXT,
    source_kind         TEXT NOT NULL CHECK(source_kind IN ('manual', 'csv')),
    import_item_id      TEXT,
    created_at          TEXT NOT NULL,
    CHECK(source_kind = 'csv' OR import_item_id IS NULL),
    FOREIGN KEY(benchmark_id) REFERENCES benchmarks(id) ON DELETE RESTRICT,
    FOREIGN KEY(import_item_id) REFERENCES import_items(id) ON DELETE RESTRICT
);

CREATE INDEX idx_benchmark_observations_selection
ON benchmark_observations(benchmark_id, observed_on DESC, created_at DESC, id DESC);

CREATE INDEX idx_benchmark_observations_import
ON benchmark_observations(import_item_id);

CREATE TABLE household_benchmark_preferences (
    household_id        TEXT PRIMARY KEY NOT NULL,
    benchmark_id        TEXT NOT NULL,
    updated_at          TEXT NOT NULL,
    FOREIGN KEY(household_id) REFERENCES households(id) ON DELETE CASCADE,
    FOREIGN KEY(benchmark_id) REFERENCES benchmarks(id) ON DELETE RESTRICT
);

CREATE INDEX idx_household_benchmark_preferences_benchmark
ON household_benchmark_preferences(benchmark_id, household_id);
