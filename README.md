# Nestworth

Nestworth is a local-first macOS application for building and maintaining a personal or household balance sheet. It tracks material assets and liabilities, exact member ownership, institutions, groups, and current values without requiring an account or an internet connection.

## Status

v0.1.1 development is complete and the project is in release-candidate validation.

- Platform: macOS 26.0 or later
- Architecture: Apple Silicon `arm64` only
- Data: local SQLite database
- Distribution: `.app` and `.dmg`
- Public release: not yet signed or notarized

The current pull request is not itself a public release. Use an isolated test database when launching locally built artifacts.

## v0.1.1 Features

- Atomic onboarding for one Household and one or more Members
- Member, Institution, and Group management with archive and restore
- Accounts for cash, investments, property, receivables, and liabilities
- Exact sole or shared ownership using basis points
- Append-only balance and manual-value observations
- Assets, liabilities, net worth, and allocation breakdowns computed in Rust
- Account views by owner, Shared ownership, category, institution, and group
- Local Member avatars and entity logos through a bounded native image pipeline
- English and Simplified Chinese with live language switching
- System, Light, and Dark appearance with live updates
- Pre-migration snapshots and blocked startup for unsupported future databases

## Current Boundaries

v0.1.1 uses one Household base currency and manual current values. It does not include multi-currency conversion, Holdings, live prices, transfers, an Activity ledger, historical charts, performance analytics, automation, import/export, or user-managed backup.

The Household name and base currency are fixed after onboarding in this release. Avatars and logos can be set or replaced but not cleared.

See the [v0.1.1 release contract](docs/releases/v0.1.1.md) for the exact delivered scope and accepted limitations.

Planning for the next release is available in the [v0.1.2 release contract](docs/releases/v0.1.2.md), [technical design](docs/releases/v0.1.2-technical-design.md), and [implementation plan](docs/releases/v0.1.2-implementation-plan.md).

## Privacy and Data Ownership

The Rust backend is the only business-data database client. Core workflows have no network dependency, and the frontend receives typed data through generated Tauri commands.

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
