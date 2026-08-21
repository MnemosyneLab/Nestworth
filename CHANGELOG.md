# Changelog

All notable changes to Nestworth are recorded here. The project follows semantic versioning once a public release is published.

## [0.1.5] - Unreleased

v0.1.5 has local Phase 12 release-candidate evidence; keyboard/VoiceOver device checks, Developer ID distribution, and release publication remain outstanding.

### Added

- Maintenance route with review-before-post pending Activities, recurring rule generation, freshness policies, snoozes, and explicit preview/post/skip workflows.
- Settings Data Management workflows for verified local Backup/Restore, recovery inspection, canonical JSON and CSV export, strict CSV preview/commit, and import history.
- Analytics Benchmark catalog, append-only observations, default selection, and Rust-computed comparison with portfolio TWR and excess return.
- Bounded Rust global search across household records and an accessible `Command+K` palette with safe static navigation actions.
- English and Simplified Chinese product copy, route-gate coverage, generated command mocks, and Phase 10 behavior tests.

### Engineering

- Added the `global_search` IPC command with escaped, bounded, deterministic Household-scoped queries and no financial-value search.
- Kept selected-file content Rust-owned; the frontend uses only the typed dialog and generated command surfaces.
- Added Phase 10 frontend smoke coverage for explicit generation and palette focus restoration.
- Added Phase 11 date-stable attribution regression coverage, full IPC operation-boundary auditing, and media/immutable-evidence Backup/Restore round-trip coverage.

## [0.1.4] - Unreleased

v0.1.4 is development-complete and is undergoing release-candidate validation. Package, Cargo, and Tauri versions are `0.1.4`. This closeout does not assign a publish date, create a git tag, or mark the product Released.

### Added

- FIFO tax lots derived from the posted v0.1.3 Activity ledger, with no lot policy configuration and no persisted lot table
- Realized and unrealized gain reported separately as gross and net of allocated fees
- Append-only cost-basis declarations that supply a cost for an unknown-basis position without creating an Activity
- Investment income and fee totals by kind, including trade commissions that carry no fee kind
- Exact currency decomposition of base-currency gain into instrument movement and currency movement
- Daily-linked time-weighted return and XIRR money-weighted return across Household, Portfolio, Account, and Instrument scopes
- Net-worth attribution bridge with an explicit unexplained residual
- Top-level `/analytics` route, lot table, unknown-basis worklist, and Investments and Account detail integration

### Engineering

- Migration `004` creates `cost_basis_declarations` only and rewrites no v0.1.3 business row
- Application-layer analytics modules of free functions, `SignedMoney`/`ReturnRate` output types, and `rust_decimal` `maths` for annualization and XIRR
- Frozen 81-command allowlist; analytics reads write nothing; declaration and revocation are the only new writes
- Golden v0.1.1, v0.1.2, and v0.1.3 fixtures keep Overview and portfolio totals after migrate to schema 4
- Zero-write coverage for unsupported future databases, including version `5`, across bootstrap, Activity/history, and analytics commands
- `delete_all_data` coverage for schema-4 databases, WAL/SHM sidecars, and `.pre-migrate-*` snapshots including a stray `.pre-migrate-3`

### Accepted v0.1.4 Limitations

- No benchmarks, index series, peer comparison, or relative return
- FIFO is the only lot policy; average-cost, specific-lot, LIFO, and HIFO remain deferred
- No tax reporting, wash-sale rules, or jurisdiction-specific cost rules
- Isolated-data launch, keyboard/VoiceOver, arm64 packaging, and signing remain named macOS release checks
- Binary floating point remains prohibited, including in the XIRR solver
- A declared basis is not an imported transaction; v0.1.5 must not backfill acquisition history from a declaration and must not silently reinterpret an existing FIFO result

### Distribution Status

- Automated frontend and Linux-host Rust gates are the development evidence for this revision
- Remaining named macOS distribution checks, not executed in this Linux closeout:
  - Isolated-data application launch with production providers unconfigured
  - Keyboard-only smoke of Analytics, lot table, declaration, attribution, and integrated Overview/Investments/Account workflows
  - VoiceOver smoke testing
  - arm64 `.app` and `.dmg` packaging with version and minimum-macOS metadata (`Nestworth_0.1.4_aarch64.dmg`)
  - Chosen signing/notarization policy; unsigned artifacts remain controlled-test only

## [0.1.3] - Unreleased

v0.1.3 is development-complete and is undergoing release-candidate validation. Package, Cargo, and Tauri versions are `0.1.3`. This closeout does not assign a publish date, create a git tag, or mark the product Released.

### Added

- Immutable Activity ledger with kind-specific posting for adjustments, deposits, withdrawals, transfers, trades, income, fees, debt principal, and manual valuations
- Atomic current-state projection of accepted Activities into Account Value, Account Cash, and Holding Quantity
- Reversal and atomic correction that preserve the original, reversal, and replacement chain
- History Origin cutover for migrated v0.1.2 Households and fresh onboarding, with timezone confirmation before the first Activity or snapshot
- Account timeline combining origin, Activities, legacy observations, and financial-state changes
- Top-level `/activity` route with URL-backed filters, cursor pagination, type-specific forms, and Rust-produced preview
- Closed-day append-only valuation snapshots, bounded rebuild, and Overview net-worth trend with a live current point
- English and Simplified Chinese copy for Activity, timeline, origin, rebuild, and trend workflows
- Cross-currency internal transfer conversion spread versus market FX at effective time, with a derived transaction rate and optional explicit fee

### Changed

- Cross-currency cash Transfer no longer accepts a user-entered transaction FX rate; Rust derives it from the two native amounts
- Manual FX quote history is listed on the Investments FX card; quote edits remain observations, not Activities

### Engineering

- Migration `003` for History Origin, Activities, legs, Quantity and state observations, and snapshot revisions without rewriting v0.1.2 business rows
- ActivityService, HistoricalValuationService, snapshot rebuild, and generated Activity/history IPC commands
- Golden v0.1.1 and v0.1.2 fixtures that keep Overview and portfolio totals after migrate to schema 3
- Zero-write coverage for unsupported future databases across bootstrap and Activity/history commands
- `delete_all_data` coverage for schema-3 databases, WAL/SHM sidecars, and `.pre-migrate-*` snapshots

### Accepted v0.1.3 Limitations

- No cost basis, tax lots, realized or unrealized gain, TWR, XIRR, benchmarks, or return attribution
- Origin and adjustment quantities have unknown basis; v0.1.4 must not manufacture lots from v0.1.2 Holding Quantity
- No live market-data or FX vendor is configured; production adapters remain unconfigured
- Pending or future Activities, CSV import/export, backup/restore, and background snapshot generation while closed remain deferred

### Distribution Status

- Automated frontend and Linux-host Rust gates are the development evidence for this revision
- Remaining named macOS distribution checks, not executed in this Linux closeout:
  - Isolated-data application launch with production providers unconfigured
  - Keyboard-only smoke of Activity, timeline, reversal/correction, and trend workflows
  - VoiceOver smoke testing
  - arm64 `.app` and `.dmg` packaging with version and minimum-macOS metadata (`Nestworth_0.1.3_aarch64.dmg`)
  - Chosen signing/notarization policy; unsigned artifacts remain controlled-test only

## [0.1.2] - Unreleased

v0.1.2 is development-complete and is undergoing release-candidate validation.

### Added

- Holdings-tracked Investment Accounts with current quantities and no invented initial Account Value
- Household-scoped Instruments with type, quote currency, optional market metadata, quote preference, and logos
- Multiple cash currencies inside one Holdings Account
- Append-only manual instrument prices and FX rates, including inverse base-currency conversion
- One Rust ValuationService shared by Overview, Account detail, and Investments
- Native amount, base-currency value, quote provenance, freshness, completeness, and unvalued diagnostics
- Investments page with portfolio total, positions, and allocation by currency, country, and instrument type
- Legacy Balance and Manual Value investment Accounts included in complete portfolio totals with explicit manual/unclassified allocation buckets
- Full-precision checked Decimal aggregation with four-place midpoint-nearest-even Money DTO rounding
- Provider-only batch refresh with deduplication, manual-pair exclusion, and partial-success reporting
- Foreign-currency Balance and Manual Value Accounts valued through explicit FX quotes
- Route-level lazy loading for primary application pages

### Engineering

- Migration `002` for instruments, holdings, account cash, instrument quotes, FX preferences, and FX quotes
- Deterministic fake quote and FX adapters for offline provider tests
- Golden CNY/SGD/USD holdings fixture totaling `62190 CNY`
- Committed sanitized v0.1.1 migration fixture with pre-migration Overview and relationship-preservation evidence
- Compatibility coverage for migrating a v0.1.1 schema without rewriting prior Account Values

### Accepted v0.1.2 Limitations

- No live market-data or FX vendor is configured; production adapters are unconfigured and manual valuation is the complete offline path
- Provider credentials and macOS Keychain storage are deferred until a vendor is selected
- Holding quantity is current state, not a trade or Activity
- FX conversion is direct or inverse against the Household base currency only
- Background, scheduled, or offline refresh is not implemented
- Cost basis, performance, Activity history, and multi-hop FX remain deferred

### Distribution Status

- Automated frontend and Linux-host Rust gates are the development evidence for this revision
- arm64 `.app` and `.dmg` packaging, Developer ID signing, notarization, isolated-data launch, and VoiceOver smoke tests remain macOS-only release checks

## [0.1.1] - Unreleased

v0.1.1 is development-complete and is undergoing release-candidate validation.

### Added

- Local-first Household onboarding with one or more Members
- Member, Institution, and Group create, edit, archive, and restore workflows
- Account creation, editing, exact Ownership, current-value updates, archive, and restore
- Backend-computed assets, liabilities, net worth, and allocation breakdowns
- URL-backed owner, Shared, category, institution, and group Account filters
- Member avatars and Institution, Group, and Account logos from local images
- Live English, Simplified Chinese, System, Light, and Dark preferences
- Pre-migration snapshots and zero-write blocking for unsupported future databases
- Production CSP, capability, generated-IPC, and log-redaction audits

### Engineering

- Bun-only frontend workflow and locked Rust toolchain
- Generated Rust-to-TypeScript Tauri command bindings
- Domain, repository, transaction, compatibility, frontend, accessibility, and golden tests
- Apple Silicon `.app` and `.dmg` build targeting macOS 26.0 or later
- MIT License with matching JavaScript and Rust manifest metadata

### Accepted v0.1.1 Limitations

- Household name and base currency are read-only after onboarding
- Avatars and logos can be set or replaced but not cleared
- Image import decodes, resizes, and re-encodes PNG/JPEG/WebP as PNG but has no separate EXIF orientation pass
- Multi-currency, Holdings, market data, Activity, history, analytics, automation, import/export, and user-managed backup are deferred

### Distribution Status

- Local arm64 `.app` and `.dmg` bundles build successfully and the DMG checksum verifies
- Artifacts are only linker/ad-hoc signed; the application bundle is not Developer ID signed
- Notarization and stapling have not been performed
- Isolated-data application launch, DMG launch, and device VoiceOver smoke tests remain required before public distribution
