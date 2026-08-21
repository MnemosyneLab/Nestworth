# v0.1.6 fixture goldens

All timestamps below are UTC and all decimal values are strings at the Rust
boundary. The fixtures are synthetic and are not evidence that Yahoo is
available in production.

| fixture | expected normalized result |
| --- | --- |
| `current-primary.json` | `NVDA`, `143.25 USD`, `2026-08-20T20:00:00.000Z`, not delayed |
| `current-fallback.json` | `QQQ`, `517.125 USD`, `2026-08-19T20:00:00.000Z`, delayed |
| `history-gaps.json` | closes on `2026-08-17` and `2026-08-19`; the null `2026-08-18` bar is omitted |
| `fx-direct.json` | `USD/CNY`, `7.1234`, `2026-08-20T16:00:00.000Z`, not delayed |

Coverage intervals use inclusive History-Origin local dates. For a request of
`2026-08-17..2026-08-19`, a successful zero-bar response still covers the
whole interval. An existing `2026-08-17..2026-08-18` interval leaves the
ordered gap `2026-08-19..2026-08-19`; adjacent intervals merge into one
closed union.

Invalid fixtures must produce no quote or coverage write:

- `invalid-cardinality.json` has two chart results;
- `array-mismatch.json` has unequal timestamp and close lengths;
- `currency-mismatch.json` returns EUR for a USD target;
- `malformed-decimal.json` contains a non-canonical non-finite price;
- `unknown-symbol.json` has a chart-level error and no result.
