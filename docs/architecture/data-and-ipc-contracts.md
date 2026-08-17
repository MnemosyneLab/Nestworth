# Data and IPC Contracts

## Ownership of Contracts

The migration files are the executable database schema. Rust command signatures and Specta derives are the executable IPC schema. This document records the stable behavior that implementations must preserve without duplicating full SQL or generated TypeScript.

## SQLite Runtime

The application stores business data in `nestworth.sqlite3` under the Tauri application data directory. Rust is the only database client.

Writable connections use:

- Foreign keys enabled
- WAL journal mode
- Normal synchronous mode
- A five-second busy timeout
- A pool capped at four connections

Startup verifies foreign-key enforcement, WAL mode, `foreign_key_check`, and `integrity_check`. A failure produces a blocked runtime instead of deleting, replacing, or silently recreating the user's database.

## Migration Compatibility State Machine

Embedded SQLx migrations define the maximum supported migration version.

| Condition | Runtime behavior |
| --- | --- |
| Database absent | Create it, run migrations, verify it, and initialize settings |
| Database version supported and current | Open and verify it |
| Database version older | Run pending migrations, then verify it |
| Database version newer than supported | Return `UnsupportedNewerDatabase` with no writable pool |
| Migration failure | Return a blocked migration state |
| Integrity or metadata failure | Return a blocked corrupt-database state |
| Path or open failure | Return a blocked unavailable state |

The migration version is inspected through a read-only connection before any writable connection is opened. An unsupported future database must receive zero application writes, including settings initialization, migrations, recovery data, or metadata changes. All business commands obtain their database through `AppState::writable_db`, so the blocked state applies uniformly.

## Current Persistence Responsibilities

| Table | Responsibility | Important integrity boundary |
| --- | --- | --- |
| `households` | Singleton balance-sheet root | Database-enforced single row and valid currency shape |
| `members` | Household people | Household FK, archive state, optional avatar reference |
| `institutions` | Account providers or locations | Household FK, archive state, optional logo reference |
| `account_groups` | User-defined organization | Household FK, archive state, optional logo reference |
| `accounts` | Account metadata and financial classification | Household and optional reference FKs, category and tracking checks, boolean checks |
| `account_ownership` | Member shares for Accounts | Composite identity, positive bounded shares, restricted Member deletion |
| `account_values` | Append-only balance or manual-value observations | Account FK, value-kind and currency checks, latest-value index |
| `media_assets` | Household-scoped binary images | Household FK and cascading Household deletion |
| `app_settings` | Singleton language, appearance, and last Household pointer | Fixed row ID and enumerated settings values |

SQLite constraints provide structural integrity. Rules requiring sums, comparisons with current state, retained archived references, or last-active-member checks remain application-service responsibilities. Database triggers are not used for v0.1.1.

## Transactions and Query Guarantees

Write use cases begin with `BEGIN IMMEDIATE`. Success commits once; any validation, lookup, SQL, or DTO assembly error rolls the entire transaction back. Creation and update flows must never leave partial Accounts, Ownership, Values, Household setup, or reference records.

Read models that combine multiple queries use one read transaction so they observe one SQLite snapshot. Overview reads the Household, Accounts, Members, Institutions, and Groups under the same transaction.

List queries are bounded by collection type, not row count. Account List loads Accounts with latest values in one query and Ownership in one batch query. Queries inside a loop over returned Accounts are prohibited.

All default lists are deterministic:

```text
sort_order ASC, name COLLATE NOCASE ASC, id ASC
```

Latest Account Value uses:

```text
effective_at DESC, created_at DESC, id DESC
```

## Mutation Guarantees

- Creation validates all input before or inside the same transaction as persistence.
- Unknown mutation targets return `NOT_FOUND` and write nothing.
- Invalid input leaves existing rows and timestamps unchanged.
- Archive and restore are idempotent and do not touch `updated_at` when no state changes.
- The final active Member cannot be archived, including under concurrent requests.
- New Account references must be active and belong to the current Household.
- Account updates may retain, but may not newly select, an archived reference.
- TrackingMode cannot change after Account creation.
- Account Value updates append a new observation; they do not overwrite prior observations.

Canonical business reasoning for these guarantees is in the [domain model](domain-model.md).

## Command Surface

The v0.1.1 command surface is grouped by use case:

| Group | Commands |
| --- | --- |
| Startup | `bootstrap` |
| Onboarding | `complete_onboarding` |
| Members | list, create, update, archive, restore |
| Institutions | list, create, update, archive, restore |
| Groups | list, create, update, archive, restore |
| Accounts | list, get, create, update, update value, archive, restore |
| Overview | `get_overview` |

Media and settings mutation commands are planned for Phase 9 and are not part of the current command surface.

Command adapters remain thin. Application services own transactions and domain conversion. Frontend code calls the generated `commands` client rather than using raw Tauri invoke names.

## DTO and Serialization Rules

- Rust structs and enums use Specta to generate TypeScript definitions.
- Struct fields cross IPC in `camelCase`.
- Error codes cross IPC in `SCREAMING_SNAKE_CASE`.
- Money amounts and other decimals cross IPC as canonical strings.
- IDs and timestamps cross IPC as strings after Rust validation.
- Optional values use explicit nullable or optional fields from the generated type.
- Frontend code must not hand-edit generated bindings.

Run binding generation after any command or DTO change, then run the binding check to detect drift. The exact commands live in the [engineering guide](../development/engineering-guide.md).

## Error Contract

Every command returns either its typed result or a `CommandError` containing:

- `code`: a stable machine-readable ErrorCode
- `message`: safe user-facing English text
- `fields`: optional structured context for forms or diagnostics

Current error categories include validation, not found, conflict, already onboarded, invalid Ownership total, base-currency change restriction, invalid Category, invalid Money, invalid Media, database error or unavailability, unsupported future database, migration failure, and internal error.

Raw SQL, filenames other than the explicit blocked-startup database path, query text, driver details, and sensitive values must not appear in frontend errors. Detailed failures are logged locally with stable event names.

## Archive References

The canonical lifecycle behavior is defined in the [domain model](domain-model.md#lifecycle-and-reference-rules). Persistence implements it by keeping archived rows and foreign keys intact, loading archived catalogs when a read model must resolve retained references, and comparing proposed Account references with the current Account inside the update transaction. Default list predicates and creation-picker DTOs omit archived rows. The same enforcement pattern applies to Member Ownership, Institution, and Group references.

## Media Contract

The current schema stores MediaAsset bytes in SQLite and references them with nullable typed IDs. Phase 9 must add a native import and read boundary with these requirements:

- Accept only explicitly supported raster image formats.
- Enforce input and decoded-size limits before persistence.
- Normalize orientation and metadata, resize to a bounded dimension, and encode to a canonical format.
- Store only Household-scoped normalized bytes and MIME metadata.
- Return display-safe data without exposing arbitrary filesystem paths.
- Replacing or clearing a reference must not delete an asset still referenced elsewhere.
- Any capability or dialog permission must be minimal and included in the Phase 9 security review.

Until that boundary exists, the presence of `media_assets` and nullable media IDs is schema preparation, not an implemented upload feature.
