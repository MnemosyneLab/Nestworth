# Changelog

All notable changes to Nestworth are recorded here. The project follows semantic versioning once a public release is published.

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
