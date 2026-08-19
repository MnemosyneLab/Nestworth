# Nestworth Documentation

This directory contains the product, architecture, engineering, and release documentation for Nestworth. It is written for contributors who need to understand the product intent, preserve financial correctness, or deliver a release without rediscovering earlier decisions.

## Documentation Map

| Document | Canonical responsibility |
| --- | --- |
| [Product Vision](product/product-vision.md) | Product problem, audience, principles, user concepts, workflows, and boundaries |
| [Product Roadmap](product/roadmap.md) | Release sequence, outcomes, dependencies, and deferred capabilities |
| [System Overview](architecture/system-overview.md) | Platform, application layers, runtime flows, technology choices, privacy, and security boundaries |
| [Domain Model](architecture/domain-model.md) | Business entities, financial semantics, validation rules, lifecycle rules, and calculations |
| [Data and IPC Contracts](architecture/data-and-ipc-contracts.md) | SQLite, migrations, transactions, query guarantees, Tauri commands, DTOs, and errors |
| [Engineering Guide](development/engineering-guide.md) | Local workflow, repository conventions, test strategy, and release checks |
| [v0.1.1 Release Contract](releases/v0.1.1.md) | Delivered scope, accepted limitations, evidence, and the public-release gate |
| [v0.1.2 Release Contract](releases/v0.1.2.md) | Delivered multi-currency and portfolio scope, locked decisions, provider gate, and release acceptance |
| [v0.1.2 Technical Design](releases/v0.1.2-technical-design.md) | Implemented multi-currency, portfolio, quote, valuation, persistence, IPC, and provider contracts |
| [v0.1.2 Implementation Plan](releases/v0.1.2-implementation-plan.md) | Dependency-ordered phases, test obligations, exit checks, and Agent handoff rules |
| [v0.1.3 Release Contract](releases/v0.1.3.md) | Delivered Activity and history scope, locked product decisions, acceptance scenarios, and compatibility promises |
| [v0.1.3 Technical Design](releases/v0.1.3-technical-design.md) | Implemented ledger, projection, History Origin, correction, snapshot, persistence, IPC, and UI contracts |
| [v0.1.3 Implementation Plan](releases/v0.1.3-implementation-plan.md) | Dependency-ordered v0.1.3 phases, required tests, exit checks, and Agent handoff rules |
| [v0.1.3 Compatibility Baseline](releases/v0.1.3-baseline.md) | Frozen schema `002`, command allowlist, Overview/Portfolio goldens, query counts, and required product decisions |
| [v0.1.4 Release Contract](releases/v0.1.4.md) | Implemented analytics and performance scope, locked product decisions, acceptance scenarios, and compatibility promises |
| [v0.1.4 Technical Design](releases/v0.1.4-technical-design.md) | Implemented lot ledger, cost basis, gain, currency decomposition, return, attribution, persistence, IPC, and UI contracts |
| [v0.1.4 Implementation Plan](releases/v0.1.4-implementation-plan.md) | Dependency-ordered v0.1.4 phases, required tests, exit checks, and Agent handoff rules |
| [v0.1.4 Compatibility Baseline](releases/v0.1.4-baseline.md) | Frozen schema `004`, 80-command allowlist, ledger facts available to analytics, goldens, decimal contract, and query counts |

[v0.1.1](releases/v0.1.1.md) remains the household balance-sheet baseline. [v0.1.2](releases/v0.1.2.md) is development-complete and in release-candidate validation; it uses manual valuation with unconfigured production quote adapters. [v0.1.3](releases/v0.1.3.md) is development-complete and in release-candidate validation; Activity ledger, History Origin, snapshots, `/activity`, and Overview trend are implemented. Isolated-data launch, keyboard/VoiceOver, arm64 packaging, and signing remain named Phase 10 macOS checks. [v0.1.4](releases/v0.1.4.md) is development-complete pending Phase 11 release closeout; phases 0–10 are implemented. Isolated-data launch, keyboard/VoiceOver, arm64 packaging, and signing remain named Phase 11 macOS checks. Package, Cargo, and Tauri versions remain `0.1.3` until Phase 11. Later direction is summarized in the [product roadmap](product/roadmap.md).

## Source of Truth

When sources disagree, use this order:

1. Current code, manifests, migrations, generated bindings, and tests define implemented behavior.
2. Explicit locked project decisions define required constraints.
3. The release contract defines delivered scope, accepted limitations, and required release evidence.
4. The roadmap expresses direction, not an implementation contract.

The documentation explains contracts but does not replace executable validation. Migration SQL owns the physical schema, Rust domain code owns enforced invariants, generated TypeScript owns the IPC wire shape, and package manifests own commands and dependency versions.

## Status Vocabulary

| Status | Meaning |
| --- | --- |
| `Implemented` | Confirmed in the current repository by code and relevant tests |
| `Planned` | Approved for the active release but not yet complete |
| `Deferred` | Intentionally outside the active release; details may change before implementation |

Do not use `Implemented` for an aspiration, a schema placeholder, or an old plan. If evidence is incomplete, use `Planned` and state the missing acceptance condition.

Feature status and release status are separate. `Development complete` means all accepted feature phases are implemented. `Release candidate` means automated gates pass but distribution operations may remain. `Released` requires a published, identified artifact and completed distribution policy.

## Maintenance Rules

- Write all documentation, filenames, diagrams, tables, and examples in English.
- Give each rule one canonical home and link to it elsewhere.
- Keep product intent independent from storage and framework details.
- Keep release status out of stable architecture documents.
- Record only current technology decisions; avoid long comparisons with rejected tools.
- Do not duplicate migration SQL, generated bindings, dependency lists, or source trees.
- Update the release contract when a phase, accepted limitation, or release gate changes status.
- Update architecture or domain documentation in the same change that alters a stable contract.
- Verify relative links, run `git diff --check`, and run the repository checks before committing documentation changes.
