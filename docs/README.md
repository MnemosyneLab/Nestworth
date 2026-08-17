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
| [v0.1.1 Release Contract](releases/v0.1.1.md) | Current release scope, phase status, remaining work, and acceptance criteria |

The active release is [v0.1.1](releases/v0.1.1.md). Future direction is summarized in the [product roadmap](product/roadmap.md); a detailed release contract is created only when a release becomes active.

## Source of Truth

When sources disagree, use this order:

1. Current code, manifests, migrations, generated bindings, and tests define implemented behavior.
2. Explicit locked project decisions define required constraints.
3. The active release contract defines approved but unfinished behavior.
4. The roadmap expresses direction, not an implementation contract.

The documentation explains contracts but does not replace executable validation. Migration SQL owns the physical schema, Rust domain code owns enforced invariants, generated TypeScript owns the IPC wire shape, and package manifests own commands and dependency versions.

## Status Vocabulary

| Status | Meaning |
| --- | --- |
| `Implemented` | Confirmed in the current repository by code and relevant tests |
| `Planned` | Approved for the active release but not yet complete |
| `Deferred` | Intentionally outside the active release; details may change before implementation |

Do not use `Implemented` for an aspiration, a schema placeholder, or an old plan. If evidence is incomplete, use `Planned` and state the missing acceptance condition.

## Maintenance Rules

- Write all documentation, filenames, diagrams, tables, and examples in English.
- Give each rule one canonical home and link to it elsewhere.
- Keep product intent independent from storage and framework details.
- Keep release status out of stable architecture documents.
- Record only current technology decisions; avoid long comparisons with rejected tools.
- Do not duplicate migration SQL, generated bindings, dependency lists, or source trees.
- Update the active release contract when a phase changes status.
- Update architecture or domain documentation in the same change that alters a stable contract.
- Verify relative links, run `git diff --check`, and run the repository checks before committing documentation changes.
