import { describe, expect, it } from "vitest";

import {
  basisPointsToPercent,
  clampShareBps,
  categoryDefaults,
  equalSplitPercents,
  formatMoney,
  percentToBasisPoints,
} from "@/features/accounts/schema";

describe("account schema helpers", () => {
  it("converts ownership percents to basis points", () => {
    expect(percentToBasisPoints("100")).toBe(10_000);
    expect(percentToBasisPoints("50")).toBe(5_000);
    expect(percentToBasisPoints("33.33")).toBe(3_333);
    expect(percentToBasisPoints("0.01")).toBe(1);
    expect(percentToBasisPoints("0")).toBeNull();
    expect(percentToBasisPoints("100.01")).toBeNull();
    expect(percentToBasisPoints("33.333")).toBeNull();
    expect(percentToBasisPoints("01")).toBeNull();
    expect(percentToBasisPoints("1.2.3")).toBeNull();
  });

  it("formats basis points back to percent strings", () => {
    expect(basisPointsToPercent(10_000)).toBe("100");
    expect(basisPointsToPercent(5_000)).toBe("50");
    expect(basisPointsToPercent(3_334)).toBe("33.34");
    expect(basisPointsToPercent(10)).toBe("0.1");
    expect(basisPointsToPercent(1)).toBe("0.01");
  });

  it("splits ownership with remainder on the first owners", () => {
    expect(equalSplitPercents(3)).toEqual(["33.34", "33.33", "33.33"]);
    expect(equalSplitPercents(2)).toEqual(["50", "50"]);
  });

  it("defaults bank accounts to balance tracking and liquid assets", () => {
    expect(categoryDefaults("bank_account")).toEqual({
      primaryCategory: "cash_equivalent",
      trackingMode: "balance",
      includeInNetWorth: true,
      includeInInvestment: false,
      includeInLiquidAssets: true,
    });
  });

  it("defaults investment accounts to holdings tracking", () => {
    expect(categoryDefaults("brokerage_account")).toEqual({
      primaryCategory: "investment",
      trackingMode: "holdings",
      includeInNetWorth: true,
      includeInInvestment: true,
      includeInLiquidAssets: false,
    });
  });

  it("clamps share basis points to 0..=10000", () => {
    expect(clampShareBps(10_000)).toBe(10_000);
    expect(clampShareBps(12_000)).toBe(10_000);
    expect(clampShareBps(-5)).toBe(0);
  });

  it("formats money from canonical amount strings", () => {
    expect(formatMoney("100000", "CNY")).toBe("CNY 100,000");
    expect(formatMoney("125000.5", "CNY")).toBe("CNY 125,000.5");
    expect(formatMoney("-1600000", "CNY")).toBe("CNY -1,600,000");
  });
});
