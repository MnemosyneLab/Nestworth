# Product Vision

## Purpose

Nestworth is a private, local-first macOS application for understanding a personal or household balance sheet. It answers a focused set of questions:

- What are our total assets, liabilities, and net worth now?
- Who owns each account, including jointly owned assets?
- Where is wealth held across institutions and user-defined groups?
- How is wealth allocated across meaningful asset categories?
- Which values are stale, and how has wealth changed over time?
- Did wealth change because of saving, market performance, or currency movement?

Nestworth manages material financial positions rather than requiring transaction-level expense tracking. The product is a household wealth ledger, not a daily budgeting application.

## Audience

The primary users are individuals, couples, and families who want a durable view of their finances without placing sensitive data in a mandatory cloud account. A one-person household uses the same model as a multi-member household.

The initial product assumes that users are willing to enter and periodically maintain important balances. Later releases reduce maintenance through prices, imports, reminders, and automation without compromising local ownership.

## Product Principles

### Household First

The Household is the root of the balance sheet. Members express people, while Ownership connects accounts to one or more members. Institutions answer where an account is held; Groups provide a separate, user-defined organizational dimension.

The storage model and navigation do not have to share the same hierarchy. An account belongs to the Household and can appear under a member, the Shared view, an institution, a group, or a category without being duplicated.

### Local First

The local SQLite database is the source of truth. Core browsing and editing must work without registration or network access. Users retain control of their data, and future integrations must degrade safely when unavailable.

Cloud sync, remote market data, and provider integrations are optional future capabilities. They must not become prerequisites for opening or maintaining local data.

### Financial Correctness Before Convenience

Money is decimal, ownership is exact, liabilities have explicit sign semantics, and financial summaries come from one backend calculation path. The application rejects ambiguous input instead of silently repairing it.

### Progressive Complexity

A simple bank account should take little effort to add. Advanced concepts such as holdings, activity entries, performance attribution, and automation appear only when the user needs them. Future sophistication must extend the core model rather than redefine Household, Member, Account, or Ownership.

## User Concepts

| Concept | User meaning |
| --- | --- |
| Household | The complete personal or family balance sheet |
| Member | A person represented in ownership and allocation views |
| Institution | A bank, broker, wallet provider, lender, or other place where an account is held |
| Group | An optional user-defined purpose or portfolio grouping, such as Emergency Fund or Retirement |
| Account | A financial container or manually valued asset or liability |
| Ownership | The exact percentage of an account attributed to each member |
| Account Value | A dated observation of an account balance or manual valuation |
| Holding | A position in an investment account, such as shares of an instrument |
| Activity | An explanation of a financial change, transfer, trade, income, or fee |
| Lot | A derived FIFO acquisition batch that records what a portion of a holding cost; lots are computed, not entered |
| Cost Basis | What a position cost, either recorded by a posted trade or declared by the user for an unknown-basis lot |

Canonical business rules for these concepts live in the [domain model](../architecture/domain-model.md).

## Primary Workflows

### Start a Household

The user creates one Household, chooses its base currency, and adds at least one Member. This establishes the valuation and ownership context for the application.

### Build the Balance Sheet

The user creates optional Institutions and Groups, then adds assets and liabilities with ownership and an initial value. The flow must support a simple account quickly while retaining the fields needed for long-term organization.

### Review Net Worth

The Overview presents assets, liabilities, net worth, and allocation by category, member, institution, and group. Archived or excluded accounts do not distort active totals.

### Browse and Maintain Accounts

The user browses all, sole-owned, and shared accounts; intersects category, institution, and group filters; opens an account; updates its value; edits metadata; and archives or restores it without losing history.

### Understand Change

Activities, snapshots, and analytics distinguish external cash flow from internal movement, market return, and currency movement. Later releases add automation on top of that history.

## Information Architecture

The product grows around these top-level destinations:

- Overview
- Accounts, including All, per-member, and Shared views
- Groups
- Institutions
- Settings
- Investments, Activity, and Analytics; Automation when its release becomes active

The interface should favor direct navigation, keyboard access, clear empty states, and explicit confirmation for consequential actions. It should not expose storage-oriented concepts unless they help the user make a financial decision.

## Product Boundaries

Nestworth is designed to provide:

- A household balance sheet and net-worth view
- Flexible ownership, institution, group, and category organization
- Manual values that remain useful when providers are unavailable
- Multi-currency valuation and investment tracking in later releases
- Explainable history and performance rather than opaque estimates
- Export, backup, and recovery paths that avoid lock-in

Nestworth is not intended to become:

- A receipt-level expense tracker or envelope-budgeting system
- A bank whose balances are authoritative over the user's records
- A trading terminal or order-execution system
- A tax filing or tax-lot optimization product in the v0.1 line
- A cloud-only service that prevents offline access
- A system that silently invents ownership, exchange rates, transaction meaning, or investment performance

## Durable Product Lessons

Existing wealth products demonstrate the value of fast balance-sheet entry, joint ownership, flexible grouping, multi-currency valuation, and local data ownership. Nestworth combines those lessons with stricter separation between current value, financial activity, and performance calculation.

The durable differentiator is not a larger feature list. It is a coherent household model that remains understandable from the first manually entered account through later portfolio analytics and automation.
