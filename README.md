# Nestworth

Nestworth is a local-first macOS application for building and maintaining a personal or household balance sheet. It tracks material assets and liabilities, exact member ownership, institutions, groups, instruments, holdings, current values, an Activity ledger, and a trustworthy history boundary without requiring an account or an internet connection.

## Status

v0.1.6 is the active development line. Phases 0–5 are implemented and Phase 6 automated closeout evidence is recorded; the local arm64 `.app`/`.dmg` build succeeds, while isolated-data smoke, Developer ID signing/notarization, and publication remain release-closeout gates. Package, Cargo, and Tauri versions are synchronized to `0.1.6`. v0.1.4 and v0.1.5 remain local release-candidate lines with their public distribution gates pending.

- Platform: macOS 26.0 or later
- Architecture: Apple Silicon `arm64` only
- Data: local SQLite database
- Distribution: `.app` and `.dmg`
- Public release: v0.1.4 is not yet signed or notarized

The current pull request is not itself a public release. Use an isolated test database when launching locally built artifacts.

## v0.1.4 Baseline

- Everything in v0.1.3: onboarding, Members, Institutions, Groups, Accounts, Ownership, Instruments, Holdings, manual quotes and FX, Overview, Investments, Activity ledger, History Origin, snapshots, `/activity`, Overview trend, media, language, and appearance
- FIFO lots derived from posted Activities, with unknown-basis origin and adjustment quantities until the user declares a cost
- Realized and unrealized gain, income and fee totals, and exact currency decomposition of base-currency gain
- Daily-linked TWR and XIRR across Household, Portfolio, Account, and Instrument scopes
- Net-worth attribution bridge with an explicit unexplained residual
- Top-level `/analytics` route with lot table, unknown-basis worklist, and Investments and Account detail integration
- English and Simplified Chinese copy for analytics, lots, declarations, availability, and method names
- Manual-only operation when no market-data provider is configured; production provider controls remain unavailable

## Current Boundaries

The Household name and base currency remain fixed after onboarding. Avatars and logos can be set or replaced but not cleared.

The implemented v0.1.4 baseline does not include Benchmarks, lot policies other than FIFO, tax reporting, a live market-data vendor, automation, import/export, or user-managed Backup. Analytics never fill a missing cost, quote, or snapshot with an estimate. FX conversion is direct or inverse against the base currency only. These v0.1.5 capabilities are planned, not implemented.

All financial totals, Activity effects, historical snapshots, lots, gains, returns, and attribution are calculated by Rust. Complete active included investment Accounts contribute their authoritative base values, including legacy Balance and Manual Value Accounts; incomplete values remain visible as diagnostics and are never treated as zero. Manual prices and FX rates are the complete offline workflow. The frontend formats returned DTOs and performs no financial arithmetic.

See the [v0.1.4 release contract](docs/releases/v0.1.4.md) for the exact delivered scope and accepted limitations.

## Privacy and Data Ownership

The Rust backend is the only business-data database client. Core workflows have no network dependency, and the frontend receives typed data through generated Tauri commands. Production quote adapters remain unconfigured in the v0.1.4 baseline.

The database is stored in the Tauri application data directory as `nestworth.sqlite3`. Before testing a local build, protect any existing database and use an isolated application-data environment. Do not use release-candidate smoke tests against the only copy of real financial data.

## Development

Required toolchain:

- Bun 1.3.14
- Rust 1.97.1 with `aarch64-apple-darwin`
- Xcode and the Tauri 2 macOS prerequisites

Install dependencies and run the desktop application:

```bash
bun install --frozen-lockfile
bun run tauri dev
```

Run the complete quality gate:

```bash
bun run check
```

Build the arm64 application and disk image:

```bash
bun run tauri:build
```

The generated local bundles are not Developer ID signed or notarized. Refer to the [engineering guide](docs/development/engineering-guide.md) before distributing them.

## Documentation

Start with the [documentation index](docs/README.md):

- [Product vision](docs/product/product-vision.md)
- [Product roadmap](docs/product/roadmap.md)
- [System overview](docs/architecture/system-overview.md)
- [Domain model](docs/architecture/domain-model.md)
- [Data and IPC contracts](docs/architecture/data-and-ipc-contracts.md)
- [Engineering guide](docs/development/engineering-guide.md)
- [v0.1.1 release contract](docs/releases/v0.1.1.md)
- [v0.1.2 release contract](docs/releases/v0.1.2.md)
- [v0.1.2 technical design](docs/releases/v0.1.2-technical-design.md)
- [v0.1.2 implementation plan](docs/releases/v0.1.2-implementation-plan.md)
- [v0.1.3 release contract](docs/releases/v0.1.3.md)
- [v0.1.3 technical design](docs/releases/v0.1.3-technical-design.md)
- [v0.1.3 implementation plan](docs/releases/v0.1.3-implementation-plan.md)
- [v0.1.4 release contract](docs/releases/v0.1.4.md)
- [v0.1.4 technical design](docs/releases/v0.1.4-technical-design.md)
- [v0.1.4 implementation plan](docs/releases/v0.1.4-implementation-plan.md)
- [v0.1.5 release contract](docs/releases/v0.1.5.md)
- [v0.1.5 technical design](docs/releases/v0.1.5-technical-design.md)
- [v0.1.5 implementation plan](docs/releases/v0.1.5-implementation-plan.md)
- [v0.1.5 compatibility baseline](docs/releases/v0.1.5-baseline.md)

## License

Nestworth is available under the [MIT License](LICENSE).
