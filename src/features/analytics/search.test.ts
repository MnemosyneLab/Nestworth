import { describe, expect, it } from "vitest";

import {
  mergeAnalyticsSearch,
  toPeriodDto,
  toScopeDto,
  validateAnalyticsSearch,
} from "@/features/analytics/search";

describe("analytics search", () => {
  it("parses scope, period, ids, dates, and cursors", () => {
    expect(
      validateAnalyticsSearch({
        scope: "account",
        accountId: "a-1",
        instrumentId: "ins-1",
        period: "custom",
        start: "2026-01-01",
        end: "2026-06-01",
        lotCursor: "lot-1",
        worklistCursor: "work-1",
        declarationCursor: "decl-1",
        extra: true,
      }),
    ).toEqual({
      scope: "account",
      accountId: "a-1",
      instrumentId: "ins-1",
      period: "custom",
      start: "2026-01-01",
      end: "2026-06-01",
      lotCursor: "lot-1",
      worklistCursor: "work-1",
      declarationCursor: "decl-1",
    });
  });

  it("drops invalid scope, period, and dates", () => {
    expect(
      validateAnalyticsSearch({
        scope: "member",
        period: "2y",
        start: "01-01-2026",
        accountId: "",
      }),
    ).toEqual({});
  });

  it("maps URL search to tagged scope and period DTOs", () => {
    expect(toScopeDto({ scope: "portfolio" })).toEqual({ kind: "portfolio" });
    expect(toScopeDto({ scope: "account", accountId: "a-1" })).toEqual({
      kind: "account",
      accountId: "a-1",
    });
    expect(toScopeDto({ scope: "instrument", instrumentId: "ins-1" })).toEqual({
      kind: "instrument",
      instrumentId: "ins-1",
    });
    expect(toPeriodDto({ period: "oneYear" })).toEqual({ kind: "oneYear" });
    expect(
      toPeriodDto({ period: "custom", start: "2026-01-01", end: "2026-02-01" }),
    ).toEqual({
      kind: "custom",
      startLocalDate: "2026-01-01",
      endLocalDate: "2026-02-01",
    });
    expect(toPeriodDto({ period: "custom", start: "2026-01-01" })).toBeNull();
  });

  it("clears lot cursors when merging a scope change", () => {
    expect(
      mergeAnalyticsSearch(
        { scope: "household", lotCursor: "c1", worklistCursor: "w1" },
        { scope: "portfolio", lotCursor: undefined, worklistCursor: undefined },
      ),
    ).toEqual({ scope: "portfolio" });
  });
});
