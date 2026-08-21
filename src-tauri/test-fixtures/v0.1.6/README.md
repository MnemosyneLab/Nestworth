# v0.1.6 market-data fixtures

These are synthetic, sanitized Yahoo chart-shaped payloads for deterministic
normalization tests. They contain no user symbols, accounts, balances,
credentials, paths, or production response bodies. HTTP status, timeout, and
body-limit cases are represented by test transports rather than network calls.

The fixture contract is intentionally narrow:

- current price uses `meta.regularMarketPrice` and falls back to the newest
  aligned daily close;
- daily history uses only aligned `timestamp` and `close` values;
- `null` closes represent a market-closed day and are omitted from quote facts;
- FX values are direct positive rates and carry the response currency metadata.

The expected values and cache interval examples are in `goldens.md`.
