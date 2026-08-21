# Product Roadmap

## Strategy

The v0.1 line develops Nestworth in dependency order:

1. Establish a trustworthy household balance sheet.
2. Value positions across currencies and instruments.
3. Explain changes through an activity ledger and history.
4. Calculate performance and attribution from that history.
5. Reduce maintenance cost through automation, import, backup, and productivity tools.

Each release must preserve local-first operation and manual fallback. A later capability may extend earlier entities, but it must not reinterpret historical money, ownership, or liability data.

## Release Sequence

### v0.1.1 — Household Balance Sheet

**Theme:** Build the household balance sheet.

**Status:** Development complete; release candidate.

Deliver one Household with Members, Institutions, Groups, Accounts, exact Ownership, manually maintained balances or valuations, and a backend-computed Overview. Include local persistence, onboarding, archive and restore, account filtering, English and Simplified Chinese UI, appearance selection, media-backed avatars and logos, and release hardening.

**Exit outcome:** A user can answer what the Household owns, owes, and is worth in one base currency.

The detailed implementation contract is [v0.1.1](../releases/v0.1.1.md).

### v0.1.2 — Multi-Currency and Portfolio

**Theme:** Know what everything is worth.

**Status:** Development complete; release candidate.

Add account and instrument currencies, FX quotes, instruments, holdings, investment-account cash, market and manual quotes, data freshness, batch refresh, and a centralized valuation service. Provider failures must never prevent startup or manual valuation. v0.1.2 ships without a live market-data vendor; manual quotes are the complete offline path.

**Exit outcome:** A Household can value cash and investment positions in the base currency while retaining native amounts and quote provenance.

The detailed scope, design, and delivery phases are defined by the [v0.1.2 release contract](../releases/v0.1.2.md), [technical design](../releases/v0.1.2-technical-design.md), and [implementation plan](../releases/v0.1.2-implementation-plan.md). The live-provider preparation gate is closed for this release with no vendor selected.

### v0.1.3 — Activity and History

**Theme:** Understand how wealth changes.

**Status:** Development complete; release candidate.

Add an activity ledger for adjustments, deposits, withdrawals, transfers, trades, income, fees, debt changes, and manual valuations. Add History Origin, historical quotes, daily valuation snapshots, net-worth trends, the `/activity` route, and account timelines. Isolated-data launch, keyboard/VoiceOver, arm64 packaging, and signing remain named Phase 10 macOS release checks.

**Exit outcome:** Internal transfers do not create false wealth changes, and the user can inspect what changed over time.

The detailed scope, design, and delivery phases are defined by the [v0.1.3 release contract](../releases/v0.1.3.md), [technical design](../releases/v0.1.3-technical-design.md), and [implementation plan](../releases/v0.1.3-implementation-plan.md). Migrated v0.1.2 current state is an explicit History Origin; the release does not fabricate earlier Activities. v0.1.4 must treat origin and adjustment quantities as unknown-basis and must not manufacture lots from v0.1.2 Holding Quantity.

### v0.1.4 — Analytics and Performance

**Theme:** Know why wealth changed.

**Status:** Development complete; release candidate.

Add FIFO cost basis and lots, realized and unrealized gain, investment income and fee totals, currency decomposition of gain, scope-relative cash-flow classification, daily-linked time-weighted return, money-weighted return as XIRR, a net-worth attribution bridge, and the `/analytics` route. Lots are derived from the v0.1.3 ledger rather than entered, and the only new persisted fact is an explicit cost-basis declaration for an unknown-basis position. Isolated-data launch, keyboard/VoiceOver, arm64 packaging, and signing remain named macOS release checks.

**Exit outcome:** Contributions, internal movement, market return, currency effects, income, and fees are separated instead of being inferred from ending value alone.

The detailed scope, design, and delivery phases are defined by the [v0.1.4 release contract](../releases/v0.1.4.md), [technical design](../releases/v0.1.4-technical-design.md), [implementation plan](../releases/v0.1.4-implementation-plan.md), and [compatibility baseline](../releases/v0.1.4-baseline.md). Benchmarks moved to v0.1.5 because no market-data vendor is selected and a manually maintained index series is a data-entry problem rather than an analytics problem. v0.1.4 remains provider-free and read-only over the ledger.

### v0.1.5 — Sustainable Long-Term Use

**Theme:** Make Nestworth easy to maintain for years.

**Status:** Phase 10 implementation complete; integrity and release closeout remain planned.

Add review-before-post pending and recurring Activities, configurable valuation-review reminders, full-fidelity local Backup/Restore, canonical JSON export, strict atomic CSV import/export, manually maintained Benchmark series and relative return, bounded global search, a command palette, and keyboard-focused workflows. Automation never posts automatically, Restore is staged and recoverable, and every workflow remains offline-capable.

**Exit outcome:** Routine maintenance requires less repeated input, stale facts are visible, the complete database is recoverable, controlled bulk input cannot bypass the ledger, and Benchmark comparison does not alter financial truth.

The detailed scope, design, and delivery phases are defined by the [v0.1.5 release contract](../releases/v0.1.5.md), [technical design](../releases/v0.1.5-technical-design.md), [implementation plan](../releases/v0.1.5-implementation-plan.md), and [compatibility baseline](../releases/v0.1.5-baseline.md). Automatic posting, arbitrary institution importers, encrypted/cloud Backup, provider-backed Benchmarks, background agents, and OS notifications remain deferred.

## Capability Matrix

| Capability | 0.1.1 | 0.1.2 | 0.1.3 | 0.1.4 | 0.1.5 |
| --- | --- | --- | --- | --- | --- |
| Household, members, and exact ownership | Yes | Preserve | Preserve | Preserve | Preserve |
| Institutions, groups, accounts, and current value | Yes | Extend | Preserve | Preserve | Preserve |
| Net worth and basic allocation | Yes | Multi-currency | Historical | Attributed | Automated upkeep |
| Multi-currency and FX | No | Yes | Historical | Attribution | Preserve |
| Instruments and holdings | No | Yes | Activity-aware | Performance-aware | Importable |
| Activity ledger and transfers | No | No | Yes | Analytics input | Automatable |
| Historical trend | No | No | Yes | Explainable | Preserve |
| Investment performance | No | No | No | Yes | Preserve |
| Cost basis and lots | No | No | No | FIFO | Importable |
| Benchmarks and relative return | No | No | No | No | Yes |
| Backup, import, export, and automation | No | No | No | No | Yes |

`Yes` describes a release outcome, not the current implementation status. Status for the active release belongs in its release contract.

## Dependency Rules

- Multi-currency valuation requires explicit quote provenance and manual fallback before portfolio totals use it.
- Holdings require Instruments and a valuation service; an Account must not impersonate an Instrument.
- Performance requires Activity and historical valuation data; it must not be estimated from initial and current values.
- Cost basis is derived from posted trades; a position with no recorded acquisition stays unknown until the user declares its cost.
- Automation produces reviewable pending financial events when real execution price, date, FX, or fee may differ.
- Backup and export include every durable component required to reconstruct the user's data.

## Deferred Beyond v0.1

Possible v0.2 directions include planning and target allocation, richer portfolio analysis, optional sync, and carefully scoped external integrations. Bank sync, broker APIs, crypto-wallet sync, statement parsing, AI-assisted import, household collaboration, plugin systems, tax reporting, and advanced risk modeling remain deferred until the local data and recovery model is mature.

Detailed behavior for v0.2 and later remains intentionally undecided. Create a new release contract when a later release becomes active, using the implemented v0.1.5 result rather than its plan as the baseline.
