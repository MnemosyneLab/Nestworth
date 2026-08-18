# Nestworth

Nestworth is a local-first macOS application for building and maintaining a personal or household balance sheet. It tracks material assets and liabilities, exact member ownership, institutions, groups, instruments, holdings, and current values without requiring an account or an internet connection.

## Status

v0.1.2 development is complete and the project is in release-candidate validation.

- Platform: macOS 26.0 or later
- Architecture: Apple Silicon `arm64` only
- Data: local SQLite database
- Distribution: `.app` and `.dmg`
- Public release: not yet signed or notarized

The current pull request is not itself a public release. Use an isolated test database when launching locally built artifacts.

## v0.1.2 Features

- Everything in v0.1.1: onboarding, Members, Institutions, Groups, Accounts, Ownership, Overview, media, language, and appearance
- Holdings-tracked Investment Accounts with current quantities and multi-currency cash
- Instruments with type, quote currency, optional market metadata, and logos
- Manual instrument prices and FX rates, including inverse conversion against the Household base currency
- Native and base-currency values with quote provenance, freshness, and incomplete-total diagnostics
- Investments page with portfolio total and allocation by currency, country, and instrument type, including legacy manual-account buckets
- Checked Rust Decimal valuation at full precision, with midpoint-nearest-even rounding only at Money DTO boundaries
- Provider refresh plumbing with partial-success reporting that targets only explicitly Provider-selected Instruments and FX pairs
- Manual-only operation when no market-data provider is configured; production provider controls remain unavailable

## Current Boundaries

The Household name and base currency remain fixed after onboarding. Avatars and logos can be set or replaced but not cleared.

v0.1.2 does not include a live market-data vendor, Activity ledger, cost basis, performance analytics, historical charts, automation, import/export, or user-managed backup. Holding quantity is current state, not a trade. FX conversion is direct or inverse against the base currency only.

All financial totals and allocations are calculated by Rust. Complete active included investment Accounts contribute their authoritative base values, including legacy Balance and Manual Value Accounts; incomplete values remain visible as diagnostics and are never treated as zero. Manual prices and FX rates are the complete offline workflow. The frontend formats returned DTOs and performs no financial arithmetic.

See the [v0.1.2 release contract](docs/releases/v0.1.2.md) for the exact delivered scope and accepted limitations.

## Privacy and Data Ownership

The Rust backend is the only business-data database client. Core workflows have no network dependency, and the frontend receives typed data through generated Tauri commands. Production quote adapters are unconfigured in this release.

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

## License

Nestworth is available under the [MIT License](LICENSE).
