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
| [v0.1.4 Compatibility Baseline](releases/v0.1.4-baseline.md) | Frozen schema `004`, 81-command allowlist, ledger facts available to analytics, goldens, decimal contract, and query counts |
| [v0.1.5 Release Contract](releases/v0.1.5.md) | Sustainable-use scope, locked product decisions, acceptance scenarios, and compatibility promises |
| [v0.1.5 Technical Design](releases/v0.1.5-technical-design.md) | Pending, recurrence, freshness, Backup/Restore, import/export, Benchmark, search, persistence, IPC, and UI contracts |
| [v0.1.5 Implementation Plan](releases/v0.1.5-implementation-plan.md) | Dependency-ordered phases, Phase 10–12 evidence, required tests, exit checks, and handoff rules |
| [v0.1.5 Compatibility Baseline](releases/v0.1.5-baseline.md) | Frozen schema `004`, baseline command surface, runtime/file boundaries, v0.1.4 goldens, and automated evidence |
| [v0.1.5 Release Evidence](releases/v0.1.5-release-evidence.md) | Exact local arm64 artifact, isolated launch, checksum, signing policy, and remaining publication gates |
| [v0.1.6 Release Contract](releases/v0.1.6.md) | Planned local market-data scope, Yahoo provider boundary, acceptance, and compatibility promises |
| [v0.1.6 Technical Design](releases/v0.1.6-technical-design.md) | Planned provider registry, Yahoo normalization, cache, persistence, history, IPC, UI, and security contracts |
| [v0.1.6 Implementation Plan](releases/v0.1.6-implementation-plan.md) | Planned dependency-ordered market-data delivery phases, tests, and exit checks |
| [v0.1.6 Compatibility Baseline](releases/v0.1.6-baseline.md) | Schema-005 and v0.1.5 compatibility constraints for the planned schema-006 release |

[v0.1.1](releases/v0.1.1.md) remains the household balance-sheet baseline. [v0.1.2](releases/v0.1.2.md), [v0.1.3](releases/v0.1.3.md), and [v0.1.4](releases/v0.1.4.md) are development-complete release-candidate lines with named macOS distribution checks remaining. [v0.1.5](releases/v0.1.5.md) has local Phase 12 release-candidate evidence; public distribution closeout remains pending. [v0.1.6](releases/v0.1.6.md) is planned market-data work and has no implementation evidence yet. No release is marked `Released` without a published, identified artifact and completed distribution policy.

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
