# Engineering Guide

## Prerequisites

Nestworth development requires:

- macOS with Apple Silicon
- Bun 1.3.14, as locked by `package.json`
- Rust 1.97.1 with the `aarch64-apple-darwin` target, as locked by `rust-toolchain.toml`
- The platform prerequisites required by Tauri 2

Use Bun for all JavaScript dependency and script operations. Do not introduce npm, pnpm, Yarn, or an additional JavaScript lockfile.

## Setup and Daily Commands

Install dependencies:

```bash
bun install
```

Run the desktop application in development:

```bash
bun run tauri dev
```

Run the frontend alone when a Tauri runtime is not required:

```bash
bun run dev
```

Run the complete quality gate:

```bash
bun run check
```

The complete gate checks version synchronization, generated IPC drift, ESLint, TypeScript, frontend tests, production frontend build, all Rust targets, Rust formatting, and Clippy with warnings denied.

The repository default Cargo target is `aarch64-apple-darwin`. On a Linux host, run Rust tests, formatting, Clippy, and IPC generation with `--target x86_64-unknown-linux-gnu` instead of the default Apple target.

Useful focused commands:

```bash
bun run lint
bun run typecheck
bun run test
bun run test:watch
bun run build
bun run rust:test
bun run fmt
bun run clippy
```

`bun run format` rewrites supported frontend and configuration files. Rust formatting is separate:

```bash
cargo fmt --manifest-path src-tauri/Cargo.toml --all
```

## Generated IPC

Rust owns command and DTO definitions. Regenerate TypeScript after changing a Specta command signature or type:

```bash
bun run ipc:generate
```

Verify that committed bindings are current:

```bash
bun run ipc:check
```

Do not edit `src/generated/tauri-bindings.ts` by hand. The `export-bindings` binary is feature-gated so normal Rust binary builds do not include it.

## Build

Build the production frontend:

```bash
bun run build
```

Build Apple Silicon application and disk-image bundles:

```bash
bun run tauri:build
```

The configured target is `aarch64-apple-darwin`, and the supported bundles are `.app` and `.dmg`. A successful compile is not a launch smoke test. Do not launch a local build against the real user database under Application Support.

## Repository Layout

```text
src/
  app/                 providers, router, and query client
  components/          shared application and UI components
  features/            feature-owned pages, forms, schemas, and tests
  generated/           generated Tauri command client and DTOs
  lib/                 narrow shared frontend utilities
  locales/             English and Simplified Chinese resources
  routes/              thin TanStack Router route components

src-tauri/
  migrations/          executable SQLite schema history
  src/domain/          business values and invariants
  src/application/     use cases, transactions, queries, and DTO assembly
  src/commands/        thin Tauri command adapters
  src/infrastructure/  database connections and bootstrap
  src/ipc.rs            exported command registry
```

Feature code should remain close to its page and tests. Shared components should be extracted only after they express the same behavior in more than one feature.

## Rust Responsibilities

Rust is authoritative for:

- Domain parsing and validation
- Money, Quantity, UnitPrice, FxRate, Ownership, Category, and lifecycle rules
- Household scoping and reference validation
- Transactions and persistence
- Financial calculations, quote selection, FX conversion, and breakdowns through ValuationService
- Activity posting, preview, reversal, correction, and current-state projection through ActivityService
- Historical reconstruction, snapshot rebuild, and net-worth trend through HistoricalValuationService and history services
- Database compatibility and integrity checks
- Media validation and normalization
- Stable command errors
- Quote and FX provider adapters; the frontend never calls a market-data endpoint

Application services return complete DTOs needed by the frontend. A command wrapper should not contain SQL or business branching.

## Frontend Responsibilities

The frontend owns:

- Routing and URL search state
- Query loading, invalidation, and safe presentation
- Form state, immediate validation, focus, and submission locking
- Localization and locale-aware display
- Appearance application
- Loading, error, empty, and blocked-startup views
- Keyboard and accessibility behavior

Use generated commands for business operations. Do not duplicate financial formulas, normalize backend data into a second source of truth, or expose raw invoke calls from feature components.

## Query and Mutation Conventions

- Query keys identify the entity and any list options such as archived visibility.
- Successful mutations invalidate every affected list, detail, bootstrap, Overview, portfolio, Activity, timeline, history-status, or quote query.
- Failed mutations preserve the current UI data and display the mapped CommandError.
- Do not use optimistic updates for balances, Ownership, quotes, Activities, snapshots, or Overview totals.
- Prevent duplicate submission while a mutation is pending.
- Keep account filters in URL state and preserve them through list-detail navigation.

## Forms and Validation

Use React Hook Form with a feature-owned Zod schema for immediate user feedback. Client validation improves interaction but never replaces Rust validation.

- Convert empty optional text to `null` at the boundary.
- Keep Money and percentages as strings until Rust parses them.
- Focus the first invalid field after client validation.
- Map structured backend fields to the corresponding control when possible.
- Preserve user input on command failure.
- Require explicit cancellation for abandoning a partially completed form.

## Localization, Appearance, and Accessibility

- All user-visible strings live in locale resources.
- English keys are the semantic baseline; Simplified Chinese must have the same key set.
- Format Money with locale-aware display while preserving the backend amount string.
- Language and appearance changes must apply without restart.
- Every critical flow must work by keyboard.
- Controls need visible focus, accessible names, and correct disabled state.
- Loading, error, and empty states must be announced or represented semantically.
- Run a VoiceOver smoke test before release.

## Testing Strategy

| Layer | Required coverage |
| --- | --- |
| Domain unit | Parsing, normalization, boundaries, category policy, Ownership, sign rules, and lifecycle transitions |
| Repository integration | Actual SQLite schema, deterministic ordering, latest-value selection, archived filtering, and reference resolution |
| Transaction | Invalid or conflicting requests produce zero partial writes; concurrency-sensitive invariants remain valid |
| Command and binding | Command errors are safe and the generated TypeScript surface matches Rust |
| Frontend | User flows, forms, pending state, errors, empty states, URL restoration, navigation context, and accessibility semantics |
| Golden | Complete Household fixtures produce exact Overview and portfolio totals, including the CNY/SGD/USD `62190` holdings case after migrate to schema 3 |
| Compatibility | Unsupported future databases remain byte-for-byte unchanged by application startup and by Activity/history commands; `delete_all_data` removes schema-3 databases, WAL/SHM, and `.pre-migrate-*` snapshots |
| Activity and history | Kind-specific posting, atomic projections, origin baseline with zero fabricated Activities, quote cutoff, snapshot revisions, current ValuationService versus historical current-point agreement, and locale key identity |

Prefer behavior assertions over implementation snapshots. Tests that claim atomicity must inspect all affected rows before and after failure.

## Definition of Done for a Phase

A phase is `Implemented` only when:

- Its user-visible workflow is complete, including loading, error, empty, and pending states.
- Domain, persistence, IPC, and UI behavior agree.
- Relevant unit, integration, transaction, frontend, and golden tests pass.
- Generated bindings and locale keys are current.
- Keyboard focus and basic accessibility behavior are verified.
- Permissions, CSP, logs, and new data paths have been reviewed.
- Destructive reset changes require an explicit UI confirmation, backend confirmation validation, complete database and sidecar deletion tests, and continued zero-write protection for unsupported future databases.
- The active release contract and any changed canonical architecture document are updated.
- `bun run check` and `git diff --check` pass.

Phase 10 is not a place to defer correctness or missing tests from an earlier feature phase.

## Release Readiness

Before publishing a release:

1. Start from a clean worktree and confirm the release PR is reviewable, no longer a draft, and approved under the chosen review policy.
2. Require a green CI check or explicitly record that the repository has no CI and who performed the equivalent clean-machine verification.
3. Run `bun install --frozen-lockfile` and `bun run check`.
4. Audit the generated command list, capabilities, CSP, native plugins, and log output.
5. Exercise onboarding, CRUD, archive and restore, Account value updates, Activity posting, Account timeline, Overview trend, filters, media, Settings, and Overview on a fresh isolated database.
6. Exercise blocked startup with a future database and verify zero writes from bootstrap and Activity/history commands.
7. Verify migration and pre-migration snapshot behavior from every supported prior schema.
8. Build with `bun run tauri:build`, verify the DMG checksum, and confirm the embedded executable is arm64 with the intended version and minimum macOS value.
9. Mount the DMG read-only and smoke-test the copied application without using the real user database.
10. Complete keyboard-only automated coverage and a device VoiceOver smoke test.
11. Choose the distribution policy. Public distribution requires Developer ID signing, signature verification, notarization, and stapling; an unsigned artifact must be labeled and limited to controlled testing.
12. Confirm the MIT License and manifest metadata, finalize the changelog date, create an annotated version tag, publish checksums, and retain the exact artifacts that were tested.

Useful local artifact checks include:

```bash
hdiutil verify src-tauri/target/aarch64-apple-darwin/release/bundle/dmg/Nestworth_0.1.3_aarch64.dmg
lipo -archs src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Nestworth.app/Contents/MacOS/nestworth
codesign --verify --deep --strict --verbose=4 src-tauri/target/aarch64-apple-darwin/release/bundle/macos/Nestworth.app
```

The final `codesign` command must succeed for a signed public artifact. A linker-generated ad-hoc signature is not a Developer ID application signature.

## Documentation Changes

Use [the documentation index](../README.md) to select the canonical owner for a rule. Update one owner and link from dependent documents. Keep all documentation in English, avoid commit hashes as durable status, and verify links whenever files move.
