CREATE TABLE cost_basis_declarations (
    id                      TEXT PRIMARY KEY NOT NULL,
    household_id            TEXT NOT NULL,
    origin_holding_id       TEXT,
    activity_leg_id         TEXT,
    instrument_id           TEXT NOT NULL,

    declared_cost           TEXT,
    declared_currency       TEXT
        CHECK(declared_currency IS NULL OR declared_currency GLOB '[A-Z][A-Z][A-Z]'),
    acquired_on             TEXT,

    revokes                 TEXT,
    is_revocation           INTEGER NOT NULL
        CHECK(is_revocation IN (0, 1)),
    note                    TEXT,
    created_at              TEXT NOT NULL,

    FOREIGN KEY(household_id)
        REFERENCES households(id)
        ON DELETE CASCADE,

    FOREIGN KEY(origin_holding_id)
        REFERENCES holdings(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(activity_leg_id)
        REFERENCES activity_legs(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(instrument_id)
        REFERENCES instruments(id)
        ON DELETE RESTRICT,

    FOREIGN KEY(revokes)
        REFERENCES cost_basis_declarations(id)
        ON DELETE RESTRICT,

    CHECK(
        (origin_holding_id IS NOT NULL AND activity_leg_id IS NULL)
        OR (origin_holding_id IS NULL AND activity_leg_id IS NOT NULL)
    ),

    CHECK(
        (
            is_revocation = 1
            AND declared_cost IS NULL
            AND declared_currency IS NULL
            AND revokes IS NOT NULL
        )
        OR (
            is_revocation = 0
            AND declared_cost IS NOT NULL
            AND declared_currency IS NOT NULL
            AND revokes IS NULL
        )
    )
);

CREATE INDEX idx_cost_basis_declarations_origin_lot
ON cost_basis_declarations(
    household_id,
    origin_holding_id,
    created_at DESC,
    id DESC
)
WHERE origin_holding_id IS NOT NULL;

CREATE INDEX idx_cost_basis_declarations_leg_lot
ON cost_basis_declarations(
    household_id,
    activity_leg_id,
    created_at DESC,
    id DESC
)
WHERE activity_leg_id IS NOT NULL;

CREATE INDEX idx_cost_basis_declarations_household
ON cost_basis_declarations(
    household_id,
    created_at DESC,
    id DESC
);
