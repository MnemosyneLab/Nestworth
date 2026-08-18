CREATE TABLE households (
    id              TEXT PRIMARY KEY NOT NULL,
    singleton_key   INTEGER NOT NULL DEFAULT 1
        UNIQUE CHECK(singleton_key = 1),
    name            TEXT NOT NULL,
    base_currency   TEXT NOT NULL
        CHECK(base_currency GLOB '[A-Z][A-Z][A-Z]'),

    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL
);

CREATE TABLE media_assets (
    id              TEXT PRIMARY KEY NOT NULL,
    household_id    TEXT NOT NULL,

    mime_type       TEXT NOT NULL,
    data            BLOB NOT NULL,

    created_at      TEXT NOT NULL,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE
);

CREATE TABLE members (
    id              TEXT PRIMARY KEY NOT NULL,
    household_id    TEXT NOT NULL,

    name            TEXT NOT NULL,
    avatar_asset_id TEXT,

    note            TEXT,
    sort_order      INTEGER NOT NULL DEFAULT 0,

    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    archived_at     TEXT,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE,

    FOREIGN KEY(avatar_asset_id)
        REFERENCES media_assets(id)
        ON DELETE SET NULL
);

CREATE INDEX idx_members_household
ON members(household_id);

CREATE TABLE institutions (
    id              TEXT PRIMARY KEY NOT NULL,
    household_id    TEXT NOT NULL,

    name            TEXT NOT NULL,
    institution_type TEXT,
    country_code    TEXT,

    website         TEXT,
    note            TEXT,

    logo_asset_id   TEXT,

    sort_order      INTEGER NOT NULL DEFAULT 0,

    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    archived_at     TEXT,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE,

    FOREIGN KEY(logo_asset_id)
        REFERENCES media_assets(id)
        ON DELETE SET NULL
);

CREATE INDEX idx_institutions_household
ON institutions(household_id);

CREATE TABLE account_groups (
    id              TEXT PRIMARY KEY NOT NULL,
    household_id    TEXT NOT NULL,

    name            TEXT NOT NULL,

    icon_key        TEXT,
    color           TEXT,

    logo_asset_id   TEXT,

    description     TEXT,

    sort_order      INTEGER NOT NULL DEFAULT 0,

    created_at      TEXT NOT NULL,
    updated_at      TEXT NOT NULL,
    archived_at     TEXT,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE,

    FOREIGN KEY(logo_asset_id)
        REFERENCES media_assets(id)
        ON DELETE SET NULL
);

CREATE INDEX idx_groups_household
ON account_groups(household_id);

CREATE TABLE accounts (
    id                          TEXT PRIMARY KEY NOT NULL,
    household_id                TEXT NOT NULL,

    institution_id              TEXT,
    group_id                    TEXT,

    name                        TEXT NOT NULL,

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

    default_currency            TEXT NOT NULL
        CHECK(default_currency GLOB '[A-Z][A-Z][A-Z]'),

    note                        TEXT,

    logo_asset_id               TEXT,

    include_in_net_worth        INTEGER NOT NULL DEFAULT 1
        CHECK(include_in_net_worth IN (0, 1)),

    include_in_investment       INTEGER NOT NULL DEFAULT 0
        CHECK(include_in_investment IN (0, 1)),

    include_in_liquid_assets    INTEGER NOT NULL DEFAULT 0
        CHECK(include_in_liquid_assets IN (0, 1)),

    opened_on                   TEXT,
    closed_on                   TEXT,

    sort_order                  INTEGER NOT NULL DEFAULT 0,

    created_at                  TEXT NOT NULL,
    updated_at                  TEXT NOT NULL,
    archived_at                 TEXT,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE,

    FOREIGN KEY(institution_id)
        REFERENCES institutions(id)
        ON DELETE SET NULL,

    FOREIGN KEY(group_id)
        REFERENCES account_groups(id)
        ON DELETE SET NULL,

    FOREIGN KEY(logo_asset_id)
        REFERENCES media_assets(id)
        ON DELETE SET NULL
);

CREATE INDEX idx_accounts_household
ON accounts(household_id);

CREATE INDEX idx_accounts_institution
ON accounts(institution_id);

CREATE INDEX idx_accounts_group
ON accounts(group_id);

CREATE INDEX idx_accounts_category
ON accounts(primary_category);

CREATE TABLE account_ownership (
    account_id      TEXT NOT NULL,
    member_id       TEXT NOT NULL,

    share_bps       INTEGER NOT NULL
        CHECK(share_bps > 0 AND share_bps <= 10000),

    PRIMARY KEY(account_id, member_id),

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE CASCADE,

    FOREIGN KEY(member_id)
        REFERENCES members(id)
        ON DELETE RESTRICT
);

CREATE INDEX idx_ownership_member
ON account_ownership(member_id);

CREATE TABLE account_values (
    id              TEXT PRIMARY KEY NOT NULL,

    account_id      TEXT NOT NULL,

    value_kind      TEXT NOT NULL
        CHECK(value_kind IN (
            'balance',
            'manual_value'
        )),

    amount          TEXT NOT NULL,

    currency        TEXT NOT NULL
        CHECK(currency GLOB '[A-Z][A-Z][A-Z]'),

    effective_at    TEXT NOT NULL,
    created_at      TEXT NOT NULL,

    FOREIGN KEY(account_id)
        REFERENCES accounts(id)
        ON DELETE CASCADE
);

CREATE INDEX idx_account_values_latest
ON account_values(
    account_id,
    effective_at DESC,
    created_at DESC
);

CREATE TABLE app_settings (
    id                  INTEGER PRIMARY KEY
        CHECK(id = 1),

    language            TEXT NOT NULL DEFAULT 'system'
        CHECK(language IN (
            'system',
            'en',
            'zh-CN'
        )),

    appearance          TEXT NOT NULL DEFAULT 'system'
        CHECK(appearance IN (
            'system',
            'light',
            'dark'
        )),

    last_household_id   TEXT,

    created_at          TEXT NOT NULL,
    updated_at          TEXT NOT NULL,

    FOREIGN KEY(last_household_id)
        REFERENCES households(id)
        ON DELETE SET NULL
);
