# Changelog

All notable changes to Nestworth are recorded here. The project follows semantic versioning once a public release is published.

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
