import { describe, expect, it } from "vitest";

import { barGeometry, lotKey, mergeByLotRef } from "@/features/analytics/model";

describe("analytics chart geometry", () => {
  it("scales coordinates without changing the DTO amount strings", () => {
    const amounts = ["160.0000", "80.0000", "100.0000", "-200.0000"];
    const geometry = barGeometry(amounts);
    expect(geometry.bars).toHaveLength(4);
    expect(geometry.viewBox).toBe("0 0 640 220");
    expect(amounts).toEqual(["160.0000", "80.0000", "100.0000", "-200.0000"]);
  });
});

describe("lot merge keys", () => {
  it("keeps fragments of the same lot ref in different accounts", () => {
    const left = {
      lotRef: { sourceKind: "acquisition" as const, sourceId: "lot-1" },
      accountId: "a-1",
    };
    const right = {
      lotRef: { sourceKind: "acquisition" as const, sourceId: "lot-1" },
      accountId: "a-2",
    };
    expect(lotKey(left)).not.toBe(lotKey(right));
    const merged = mergeByLotRef(
      [
        {
          ...left,
          instrumentId: "ins-1",
          acquiredAt: "2026-01-04T00:00:00.000Z",
          quantityRemaining: "1",
          originalQuantity: "2",
          cost: { kind: "unavailable", reason: "UNKNOWN_BASIS", blockingDates: [] },
          basis: "unknown",
          isDeclared: false,
          currentValue: {
            kind: "unavailable",
            reason: "ANALYTICS_INPUT_INCOMPLETE",
            blockingDates: [],
          },
          unrealizedGross: {
            kind: "unavailable",
            reason: "UNKNOWN_BASIS",
            blockingDates: [],
          },
        },
      ],
      [
        {
          ...right,
          instrumentId: "ins-1",
          acquiredAt: "2026-01-04T00:00:00.000Z",
          quantityRemaining: "1",
          originalQuantity: "2",
          cost: { kind: "unavailable", reason: "UNKNOWN_BASIS", blockingDates: [] },
          basis: "unknown",
          isDeclared: false,
          currentValue: {
            kind: "unavailable",
            reason: "ANALYTICS_INPUT_INCOMPLETE",
            blockingDates: [],
          },
          unrealizedGross: {
            kind: "unavailable",
            reason: "UNKNOWN_BASIS",
            blockingDates: [],
          },
        },
      ],
    );
    expect(merged).toHaveLength(2);
  });
});
