import { describe, expect, it } from "vitest";

import {
  mergeActivitySearch,
  validateActivitySearch,
} from "@/features/activity/search";

describe("activity search filters", () => {
  it("parses non-empty filter and cursor params", () => {
    expect(
      validateActivitySearch({
        accountId: "a-1",
        instrumentId: "ins-1",
        kind: "deposit",
        classification: "external_inflow",
        start: "2026-01-01",
        end: "2026-08-18",
        cursor: "c1",
        extra: 1,
      }),
    ).toEqual({
      accountId: "a-1",
      instrumentId: "ins-1",
      kind: "deposit",
      classification: "external_inflow",
      start: "2026-01-01",
      end: "2026-08-18",
      cursor: "c1",
    });
  });

  it("drops empty values and invalid dates", () => {
    expect(
      validateActivitySearch({
        accountId: "",
        start: "18-08-2026",
        end: "not-a-date",
        cursor: "",
      }),
    ).toEqual({});
  });

  it("clears cursor when merging filter changes", () => {
    expect(
      mergeActivitySearch(
        { accountId: "a-1", cursor: "c1" },
        { kind: "buy", cursor: undefined },
      ),
    ).toEqual({
      accountId: "a-1",
      kind: "buy",
    });
  });
});
