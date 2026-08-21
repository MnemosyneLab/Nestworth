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
| Database version older | Copy a sibling `{filename}.pre-migrate-{found}` snapshot, including `-wal` and `-shm` sidecars when present; if that copy fails, block with no migration. Then run pending migrations and verify. |
| Database version newer than supported | Return `UnsupportedNewerDatabase` with no writable pool |
| Migration failure | Return a blocked migration state |
| Integrity or metadata failure | Return a blocked corrupt-database state |
| Path or open failure | Return a blocked unavailable state |

The migration version is inspected through a read-only connection before any writable connection is opened. An existing database that still needs a migration is copied to a sibling snapshot first; a failed snapshot copy blocks startup without running migrations. An unsupported future database must receive zero application writes, including settings initialization, migrations, recovery data, snapshot copies of that file, or metadata changes. All business commands obtain their database through `AppState::writable_db`, so the blocked state applies uniformly.

## Current Persistence Responsibilities

| Table | Responsibility | Important integrity boundary |
| --- | --- | --- |
| `households` | Singleton balance-sheet root | Database-enforced single row and valid currency shape |
| `members` | Household people | Household FK, archive state, optional avatar reference |
| `institutions` | Account providers or locations | Household FK, archive state, optional logo reference |
| `account_groups` | User-defined organization | Household FK, archive state, optional logo reference |
| `accounts` | Account metadata and financial classification | Household and optional reference FKs, category and tracking checks, boolean checks |
| `account_ownership` | Member shares for Accounts | Composite identity, positive bounded shares, restricted Member deletion |
| `account_values` | Append-only balance or manual-value observations | Account FK, value-kind and currency checks, latest-value index, nullable Activity FK after schema 003 |
| `instruments` | Household-scoped tradable or valued assets | Household FK, quote currency, quote preference, optional unique provider identity |
| `holdings` | Current Instrument quantity in a Holdings Account | Account and Instrument FKs, unique active Instrument per Account |
| `account_cash_values` | Append-only cash-by-currency observations | Account FK, currency checks, latest-value index, nullable Activity FK after schema 003 |
| `instrument_quotes` | Append-only Instrument prices | Instrument FK, source, quote time, latest-quote index |
| `fx_quote_preferences` | Manual or provider preference per unordered pair | Household FK and canonical currency pair |
| `fx_quotes` | Append-only FX observations | Household FK, distinct currencies, source, quote time |
| `media_assets` | Household-scoped binary images | Household FK and cascading Household deletion |
| `app_settings` | Singleton language, appearance, and last Household pointer | Fixed row ID and enumerated settings values |
| `history_origins` | One History Origin per Household, timezone confirmation, and cutover metadata | Unique Household, immutable financial baseline, unconfirmed timezone mutable only before the first Activity or snapshot |
| `history_origin_account_values` | Baseline Balance and Manual Value amounts | Origin and Account FKs |
| `history_origin_cash_values` | Baseline Holdings cash by Account and currency | Origin and Account FKs |
| `history_origin_holdings` | Baseline Holding Quantity and active state | Origin, Holding, Account, and Instrument FKs; not an Activity |
| `history_origin_account_states` | Baseline Category, inclusion, archive, Institution, and Group | Origin and Account FKs |
| `history_origin_ownership` | Exact baseline Ownership rows | Origin, Account, and Member FKs |
| `activities` | Immutable Activity headers and correction links | Household FK, unique reversal target, RESTRICT correction links |
| `activity_legs` | Immutable additive typed Account, cash, or Quantity effects | Exactly one component shape, RESTRICT Account/Holding/Instrument evidence |
| `holding_quantity_values` | Append-only Quantity observations | Holding FK, nullable Activity FK |
| `account_state_observations` | Append-only valuation-relevant Account state | Account FK, latest-at-cutoff index |
| `account_state_ownership` | Ownership snapshot for an Account state observation | Observation and Member FKs |
| `holding_state_observations` | Append-only Holding active or archive state | Holding FK |
| `instrument_preference_observations` | Effective-dated Manual or provider Instrument preference | Instrument FK |
| `fx_preference_observations` | Effective-dated normalized-pair FX preference | Household FK and canonical pair |
| `daily_valuation_snapshots` | Append-only daily snapshot revisions | Household FK, date and revision selection |
| `daily_valuation_snapshot_items` | Component values, provenance, and missing diagnostics | Snapshot FK, Account and Instrument lookup |
| `history_snapshot_state` | Earliest dirty local date and rebuild progress | One row per Household |
| `cost_basis_declarations` | Append-only user-supplied cost for one unknown-basis lot | Household FK; mutually exclusive `origin_holding_id` (`holdings.id`) and `activity_leg_id`; Instrument FK; currency check; revocation flag |

SQLite constraints provide structural integrity. Rules requiring sums, comparisons with current state, retained archived references, last-active-member checks, quote selection, or valuation remain application-service responsibilities. Database triggers are not used.

Schema 003 creates tables only. History Origin capture and first opening observations (quantity, Account/Holding state, and preferences with `activity_id` NULL and timestamps equal to `origin_at`) are written in a later `BEGIN IMMEDIATE` transaction after migrate, verify, and settings. Empty databases defer that origin to onboarding. Migration SQL does not rewrite v0.1.2 business rows.

Schema 004 creates `cost_basis_declarations` only. It adds no column to `activities`, `activity_legs`, or any snapshot table. Lots, gains, returns, and attribution results are not persisted; they are recomputed from one consistent read transaction.

## Transactions and Query Guarantees

Write use cases begin with `BEGIN IMMEDIATE`. Success commits once; any validation, lookup, SQL, or DTO assembly error rolls the entire transaction back. Creation and update flows must never leave partial Accounts, Ownership, Values, Household setup, or reference records.

Read models that combine multiple queries use one read transaction so they observe one SQLite snapshot. Overview and portfolio reads load Accounts, Holdings, cash, quotes, and FX under the same transaction.

List queries are bounded by collection type, not row count. Account List loads Accounts with latest values in one query and Ownership in one batch query. Queries inside a loop over returned Accounts are prohibited.

All default lists are deterministic:

```text
sort_order ASC, name COLLATE NOCASE ASC, id ASC
```

Latest Account Value and Account Cash use:

```text
effective_at DESC, created_at DESC, id DESC
```

Latest Instrument Quote and FX Quote use:

```text
quoted_at DESC, created_at DESC, id DESC
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
- Account Value, Account Cash, and Holding Quantity updates append a new observation; they do not overwrite prior observations. After History Origin, those user-facing mutations post through ActivityService and cannot bypass the ledger.
- Posted Activities are immutable. Reversal and correction append linked records; there is no edit or delete command.
- Daily snapshots are append-only revisions. Rebuild never mutates or deletes a previous revision.
- Instrument Quote and FX Quote writes are append-only.
- Refresh performs provider I/O without a SQLite write transaction and persists each successful normalized quote in its own short write transaction. A failed item does not remove manual data or previous successful quotes.

All valuation calculations remain checked Rust `Decimal` values through aggregation. Only Money DTO boundaries round to four fractional digits using midpoint-nearest-even. Frontend code formats returned strings and never calculates totals, reciprocal FX rates, or allocation percentages.

Canonical business reasoning for these guarantees is in the [domain model](domain-model.md).

## Command Surface

The command surface is grouped by use case:

| Group | Commands |
| --- | --- |
| Startup | `bootstrap` |
| Onboarding | `complete_onboarding` |
| Members | list, create, update, archive, restore, `set_member_avatar` |
| Institutions | list, create, update, archive, restore, `set_institution_logo` |
| Groups | list, create, update, archive, restore, `set_group_logo` |
| Accounts | list, get, create, update, update value, archive, restore, `set_account_logo` |
| Overview | `get_overview` |
| Instruments | list, get, create, update, archive, restore, `set_instrument_logo` |
| Holdings | list, create, update, archive, restore |
| Account cash | list, append |
| Quotes | list instrument quotes, append manual instrument quote, set instrument quote preference, list required FX, list FX quotes, append manual FX quote, set FX quote preference |
| Portfolio | `get_portfolio` |
| Market data | `get_market_data_capabilities`, search provider instruments, current refresh commands, `backfill_instrument_history`, `backfill_required_fx_history`, `backfill_all_history` |
| Media | `get_media` |
| Settings | `get_settings`, `update_settings`, `delete_all_data` |
| Origin | `get_history_origin`, `confirm_history_timezone` |
| Activities | `preview_activity`, `list_activities`, `get_activity`, `create_activity`, `reverse_activity`, `correct_activity` |
| Pending and recurring | `create_pending_activity`, `update_pending_activity`, `list_pending_activities`, `preview_pending_activity`, `post_pending_activity`, `skip_pending_activity`, recurring rule CRUD, `generate_due_pending_activities` |
| Maintenance | `list_maintenance_items`, `list_freshness_policies`, `update_freshness_policy`, `snooze_maintenance_item` |
| Backup and restore | `create_backup`, `inspect_backup`, `list_recovery_backups`, `inspect_recovery_backup`, `restore_backup` |
| Export and import | `export_canonical_json`, `export_csv`, `preview_csv_import`, `commit_csv_import`, `list_import_batches`, `get_import_batch` |
| Benchmarks | `list_benchmarks`, `create_benchmark`, `update_benchmark`, archive/restore, observations, default selection, comparison |
| Search | `global_search` |
| Timeline | `get_account_timeline` |
| History | `get_history_status`, `rebuild_history_snapshots`, `get_net_worth_trend` |
| Analytics | `get_analytics_status`, `get_performance_summary`, `get_gain_summary`, `list_holding_gain_summaries`, `get_net_worth_attribution`, `list_holding_lots`, `list_unknown_basis_lots`, `list_cost_basis_declarations`, `declare_lot_cost_basis`, `revoke_lot_cost_basis` |

The application does not expose `get_household`, `update_household`, or media-clear commands. Household name and base currency are displayed from bootstrap; language and appearance are the mutable settings. `delete_all_data` is an explicitly confirmed destructive reset: it is available only for a writable supported database, closes SQLite, removes the database, WAL/SHM sidecars, and pre-migration snapshots including schema-4 `.pre-migrate-*` files, and restarts into onboarding. It cannot delete an unsupported future-version database. Activity commands accept tagged kind-specific inputs only; `preview_activity` performs no writes. Analytics reads are read-only over the ledger; `declare_lot_cost_basis` and `revoke_lot_cost_basis` are the only analytics writes and persist no lot, gain, or valuation state. Market-data capability discovery is local and read-only. The v0.1.6 UI exposes the compiled Yahoo provider only after capability discovery, keeps symbol binding manual because search is unsupported, and sends current refresh or bounded daily backfill only after an explicit user action. Provider failures remain safe partial results and do not block local valuation or manual quotes.

The current generated IPC and frozen capability allowlist contain 122 commands. The v0.1.5 compatibility surface contains 118 commands; v0.1.6 adds capability discovery and three bounded history-backfill commands. `src-tauri/src/capabilities_test.rs` asserts that the Rust registry and generated TypeScript invoke names remain identical.

Command adapters remain thin. Application services own transactions and domain conversion. Frontend code calls the generated `commands` client rather than using raw Tauri invoke names.

## DTO and Serialization Rules

- Rust structs and enums use Specta to generate TypeScript definitions.
- Struct fields cross IPC in `camelCase`.
- Error codes cross IPC in `SCREAMING_SNAKE_CASE`.
- Money amounts and other decimals cross IPC as canonical strings.
- `FxPairStatusDto.selectedQuote` preserves the stored quote orientation; `selectedRate` is Rust-normalized to the displayed `1 currencyB = rate currencyA` direction so the frontend never calculates a reciprocal.
- IDs and timestamps cross IPC as strings after Rust validation.
- Optional values use explicit nullable or optional fields from the generated type.
- Frontend code must not hand-edit generated bindings.

Run binding generation after any command or DTO change, then run the binding check to detect drift. The exact commands live in the [engineering guide](../development/engineering-guide.md).

## Compatibility Evidence

The repository contains deterministic sanitized released fixtures at `src-tauri/test-fixtures/`. Migration tests cover the schema-005 to schema-006 upgrade, preserve representative rows and archived references, capture the History Origin baseline, reopen idempotently, and pass `foreign_key_check` and `integrity_check`. Unsupported future-version tests for schema `007` verify zero application writes from bootstrap, Activity/history commands, and analytics commands.

## Error Contract

Every command returns either its typed result or a `CommandError` containing:

- `code`: a stable machine-readable ErrorCode
- `message`: safe user-facing English text
- `fields`: optional structured context for forms or diagnostics

Current error categories include validation, not found, conflict, already onboarded, invalid Ownership total, base-currency change restriction, invalid Category, invalid Money, invalid Quantity, invalid UnitPrice, invalid FxRate, decimal overflow, duplicate Holding, quote unavailable, incomplete valuation, unsupported provider symbol, provider authentication, rate limit, provider unavailable, malformed provider response, unsupported market-data operation, oversized market-data response, invalid or unavailable daily-history range, invalid Media, database error or unavailability, unsupported future database, migration failure, history origin initialization failure, history timezone confirmation required, invalid Activity, insufficient balance or Quantity, transfer or trade mismatch, already reversed, not correctable, snapshot rebuild required or failed, analytics period unavailable, analytics input incomplete, return not computable, invalid cost-basis declaration, cost-basis lot not found, and internal error.

Raw SQL, filenames other than the explicit blocked-startup database path, query text, driver details, credentials, raw provider payloads, and sensitive values must not appear in frontend errors. Detailed failures are logged locally with stable event names.

## Archive References

The canonical lifecycle behavior is defined in the [domain model](domain-model.md#lifecycle-and-reference-rules). Persistence implements it by keeping archived rows and foreign keys intact, loading archived catalogs when a read model must resolve retained references, and comparing proposed Account references with the current Account inside the update transaction. Default list predicates and creation-picker DTOs omit archived rows. The same enforcement pattern applies to Member Ownership, Institution, and Group references.

## Media Contract

MediaAsset bytes are stored in SQLite and referenced with nullable typed IDs. The native import and read boundary is:

- Accept PNG, JPEG, and WebP input up to 5 MB.
- Decode safely, resize to at most 512×512, and encode as PNG. Avatars are center-cropped to square before downscale; logos keep aspect ratio and are not upscaled.
- Store only Household-scoped normalized bytes and MIME metadata.
- Return `{ mimeType, data }` as a display-safe base64 payload without exposing arbitrary filesystem paths.
- Choose files through the native dialog (`dialog:allow-open` only). Rust reads the selected path; the frontend has no filesystem plugin.
- Replacing a reference deletes the previous asset only when nothing else still references it.
- Invalid, oversized, or undecodable input returns `MEDIA_INVALID` and writes nothing.

v0.1.2 can set and replace avatars and logos, including Instrument logos. It does not clear a media reference, and it does not rewrite EXIF orientation as a separate metadata-stripping pass beyond decode and PNG encode.
