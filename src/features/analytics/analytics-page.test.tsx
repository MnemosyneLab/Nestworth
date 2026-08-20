import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { router } from "@/app/router";
import type {
  AnalyticsStatusDto,
  CostBasisDeclarationIpcDto,
  GainSummaryIpcDto,
  HoldingLotDto,
  InstrumentRecordDto,
  NetWorthAttributionIpcDto,
  PerformanceSummaryDto,
  SignedMoneyDto,
} from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import {
  accountRecord,
  commandError,
  deferred,
  renderReadyApp,
  resetApp,
} from "@/test/app-harness";

const TIMESTAMP = "2026-01-02T00:00:00.000Z";

describe("analytics page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("places Analytics in primary navigation", async () => {
    await renderReadyApp();
    const nav = screen.getByRole("navigation", { name: "Household" });
    expect(within(nav).getByRole("link", { name: "Analytics" })).toHaveAttribute(
      "href",
      "/analytics",
    );
  });

  it("renders chart DTO strings in the sibling attribution table", async () => {
    mockAnalyticsCatalog();
    mockAvailableAnalytics();
    await openAnalytics();
    const table = await screen.findByRole("table", { name: "Attribution bridge" });
    expect(within(table).getByText("CNY 10,000.0000")).toBeInTheDocument();
    expect(within(table).getByText("CNY 5,900.0000")).toBeInTheDocument();
    expect(within(table).getByText("CNY 1,800.0000")).toBeInTheDocument();
    expect(within(table).getByText("CNY 500.0000")).toBeInTheDocument();
    expect(within(table).getByText("CNY -200.0000")).toBeInTheDocument();
    expect(within(table).getByText("CNY -100.0000")).toBeInTheDocument();
    expect(within(table).getByText("CNY 100.0000")).toBeInTheDocument();
    expect(within(table).getByText("Unexplained")).toBeInTheDocument();
    expect(document.querySelector("svg[aria-hidden='true']")).not.toBeNull();
    expect(screen.queryByText("4.444%")).not.toBeInTheDocument();
  });

  it("explains unavailable returns with reason and dates instead of zero", async () => {
    mockAnalyticsCatalog();
    vi.mocked(commands.getPerformanceSummary).mockResolvedValue({
      status: "ok",
      data: {
        twr: {
          kind: "unavailable",
          reason: "ANALYTICS_PERIOD_UNAVAILABLE",
          blockingDates: ["2026-01-03", "2026-01-04"],
        },
        xirr: {
          kind: "unavailable",
          reason: "RETURN_NOT_COMPUTABLE",
          blockingDates: ["2026-01-03"],
        },
      },
    });
    await openAnalytics();
    expect(
      await screen.findAllByText(/required closed-day snapshots are missing or incomplete/),
    ).not.toHaveLength(0);
    expect(screen.getByText("Blocking dates: 2026-01-03, 2026-01-04")).toBeInTheDocument();
    expect(
      screen.getAllByText(/cannot be computed from the available cash-flow series/).length,
    ).toBeGreaterThan(0);
    const table = screen.getByRole("table", { name: "Return results" });
    expect(within(table).queryByText("0")).not.toBeInTheDocument();
    expect(within(table).queryByText("0.000000")).not.toBeInTheDocument();
    expect(within(table).queryByText("CNY 0")).not.toBeInTheDocument();
  });

  it("shows XIRR as an annual rate rather than a cumulative return", async () => {
    mockAnalyticsCatalog();
    mockAvailableAnalytics();
    await openAnalytics();
    const table = await screen.findByRole("table", { name: "Return results" });
    expect(within(table).getByText("0.100000")).toBeInTheDocument();
    expect(
      screen.getByText(
        /XIRR is the annual money-weighted rate/,
      ),
    ).toBeInTheDocument();
    expect(screen.getAllByText(/current snapshot/).length).toBeGreaterThan(0);
    expect(
      screen.getByText(
        /Realized currency movement uses the selected period/,
      ),
    ).toBeInTheDocument();
  });

  it("shows unknown-basis lots and explains their exclusion", async () => {
    mockAnalyticsCatalog();
    mockAvailableAnalytics();
    await openAnalytics();
    expect(
      await screen.findByText(
        "Unknown-basis lots are excluded from gain. They are listed in the unknown-basis worklist.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getAllByText(
        "Unknown-basis positions are excluded from gain totals. Their quantity and value are reported separately rather than treated as zero cost.",
      ).length,
    ).toBeGreaterThan(0);
    const worklist = screen.getByRole("table", { name: "Unknown-basis lots" });
    expect(within(worklist).getByText("Unknown")).toBeInTheDocument();
    expect(within(worklist).getByText("3")).toBeInTheDocument();
    const gain = screen.getByRole("table", { name: "Gain results" });
    expect(within(gain).getByText("3")).toBeInTheDocument();
    expect(within(gain).getByText("CNY 14,490.0000")).toBeInTheDocument();
  });

  it("requires declaration confirmation and keeps input after failure", async () => {
    const user = userEvent.setup();
    mockAnalyticsCatalog();
    mockAvailableAnalytics();
    vi.mocked(commands.declareLotCostBasis).mockResolvedValue({
      status: "error",
      error: commandError(
        "INVALID_COST_BASIS_DECLARATION",
        "This cost-basis declaration is not valid.",
      ),
    });
    await openAnalytics();
    const worklist = await screen.findByRole("table", { name: "Unknown-basis lots" });
    const declare = within(worklist).getByRole("button", {
      name: "Declare cost basis",
    });
    await user.click(declare);
    expect(
      screen.getByText(
        "This declaration does not change net worth, quantity, or history. It only supplies cost metadata for this unknown-basis lot.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Declarations are append-only. They are revoked rather than edited."),
    ).toBeInTheDocument();
    expect(
      screen.getByText(/Providing an acquisition date enables currency decomposition/),
    ).toBeInTheDocument();
    const form = screen.getByRole("heading", { name: "Declare cost basis" }).closest(
      "form",
    )!;
    await user.type(within(form).getByLabelText("Declared cost"), "1500");
    await user.click(within(form).getByRole("button", { name: "Declare cost basis" }));
    expect(commands.declareLotCostBasis).not.toHaveBeenCalled();
    await user.click(within(form).getByRole("button", { name: "Cancel" }));
    expect(within(form).getByLabelText("Declared cost")).toHaveValue("1500");
    await user.click(within(form).getByRole("button", { name: "Declare cost basis" }));
    await user.click(within(form).getByRole("button", { name: "Confirm declaration" }));
    expect(
      await screen.findByText("This cost-basis declaration is not valid."),
    ).toBeInTheDocument();
    expect(within(form).getByLabelText("Declared cost")).toHaveValue("1500");
  });

  it("restores focus after cancelling declaration and requires revocation confirmation", async () => {
    const user = userEvent.setup();
    mockAnalyticsCatalog();
    mockAvailableAnalytics();
    await openAnalytics();
    const worklist = await screen.findByRole("table", { name: "Unknown-basis lots" });
    const declare = within(worklist).getByRole("button", {
      name: "Declare cost basis",
    });
    await user.click(declare);
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(declare).toHaveFocus();

    await user.selectOptions(screen.getByLabelText("Instrument"), "ins-qqq");
    await waitFor(() => {
      expect(router.state.location.search).toEqual(
        expect.objectContaining({ instrumentId: "ins-qqq" }),
      );
    });
    const lots = await screen.findByRole("table", { name: "Holding lots" });
    const revoke = within(lots).getByRole("button", { name: "Revoke declaration" });
    await user.click(revoke);
    expect(commands.revokeLotCostBasis).not.toHaveBeenCalled();
    expect(screen.getByText("The lot returns to unknown basis.")).toBeInTheDocument();
    expect(
      screen.getByText(/Revoke the effective declaration for this lot/),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(revoke).toHaveFocus();
    await router.navigate({ to: "/analytics", search: {}, replace: true });
  });

  it("disables double submit while declaring", async () => {
    const user = userEvent.setup();
    mockAnalyticsCatalog();
    mockAvailableAnalytics();
    const pending = deferred<{
      status: "ok";
      data: CostBasisDeclarationIpcDto;
    }>();
    await openAnalytics();
    await screen.findByText("History origin holding");
    const worklist = await screen.findByRole("table", { name: "Unknown-basis lots" });
    await user.click(
      within(worklist).getByRole("button", { name: "Declare cost basis" }),
    );
    const form = screen.getByRole("heading", { name: "Declare cost basis" }).closest(
      "form",
    )!;
    await user.type(within(form).getByLabelText("Declared cost"), "1500");
    await user.click(within(form).getByRole("button", { name: "Declare cost basis" }));
    vi.mocked(commands.declareLotCostBasis).mockImplementation(() => pending.promise);
    const confirm = within(form).getByRole("button", { name: "Confirm declaration" });
    await user.click(confirm);
    expect(confirm).toBeDisabled();
    await user.click(confirm);
    expect(commands.declareLotCostBasis).toHaveBeenCalledTimes(1);
    const overviewCalls = vi.mocked(commands.getOverview).mock.calls.length;
    pending.resolve({
      status: "ok",
      data: {
        id: "decl-1",
        householdId: "hh-1",
        lotRef: unknownLot().lotRef,
        instrumentId: "ins-qqq",
        declaredCost: "1500",
        declaredCurrency: "USD",
        acquiredOn: null,
        revokes: null,
        isRevocation: false,
        note: null,
        createdAt: TIMESTAMP,
      },
    });
    await waitFor(() => {
      expect(screen.queryByRole("heading", { name: "Declare cost basis" })).toBeNull();
    });
    expect(vi.mocked(commands.getOverview).mock.calls.length).toBe(overviewCalls);
  });

  it("keeps URL scope and period across navigation", async () => {
    const user = userEvent.setup();
    mockAnalyticsCatalog();
    mockAvailableAnalytics();
    await renderReadyApp();
    await router.navigate({
      to: "/analytics",
      search: { scope: "account", accountId: "a-1", period: "oneYear" },
      replace: true,
    });
    expect(await screen.findByRole("heading", { name: "Analytics" })).toBeInTheDocument();
    expect(screen.getByRole("radio", { name: "Account" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    expect(screen.getByLabelText("Account")).toHaveValue("a-1");
    expect(screen.getByRole("radio", { name: "1 year" })).toHaveAttribute(
      "aria-checked",
      "true",
    );
    await waitFor(() => {
      expect(commands.getPerformanceSummary).toHaveBeenCalledWith({
        scope: { kind: "account", accountId: "a-1" },
        period: { kind: "oneYear" },
      });
      expect(commands.getGainSummary).toHaveBeenCalledWith({
        scope: { kind: "account", accountId: "a-1" },
        period: { kind: "oneYear" },
      });
    });
    await user.click(screen.getByRole("radio", { name: "3 months" }));
    await waitFor(() => {
      expect(router.state.location.search).toEqual(
        expect.objectContaining({ period: "threeMonths", scope: "account" }),
      );
      expect(commands.getGainSummary).toHaveBeenCalledWith({
        scope: { kind: "account", accountId: "a-1" },
        period: { kind: "threeMonths" },
      });
      expect(commands.getPerformanceSummary).toHaveBeenCalledWith({
        scope: { kind: "account", accountId: "a-1" },
        period: { kind: "threeMonths" },
      });
      expect(commands.getNetWorthAttribution).toHaveBeenCalledWith({
        scope: { kind: "account", accountId: "a-1" },
        period: { kind: "threeMonths" },
      });
    });
    router.history.back();
    await waitFor(() => {
      expect(router.state.location.search).toEqual(
        expect.objectContaining({ period: "oneYear", scope: "account" }),
      );
    });
    await router.navigate({ to: "/analytics", search: {}, replace: true });
  });

  it("labels zero realized, unknown basis, and missing input distinctly", async () => {
    mockAnalyticsCatalog();
    mockAvailableAnalytics();
    vi.mocked(commands.getGainSummary).mockResolvedValue({
      status: "ok",
      data: {
        ...availableGain(),
        basisComplete: true,
        inputComplete: true,
        realizedGross: availableSigned("0.0000"),
        realizedNet: availableSigned("0.0000"),
        allocatedFees: availableSigned("0.0000"),
        unrealizedGross: availableSigned("80.0000"),
        unknownBasisQuantity: "0",
        unknownBasisValue: {
          kind: "available",
          value: { amount: "0.0000", currency: "CNY" },
        },
        income: [],
        fees: [],
      },
    });
    await openAnalytics();
    const known = await screen.findByRole("table", { name: "Gain results" });
    expect(within(known).getAllByText("USD 0.0000").length).toBeGreaterThan(0);
    expect(within(known).queryByText("Unknown")).not.toBeInTheDocument();
    expect(
      within(known).queryByText(
        "This amount is unavailable because the lot has unknown cost basis.",
      ),
    ).not.toBeInTheDocument();

    vi.mocked(commands.getGainSummary).mockResolvedValue({
      status: "ok",
      data: {
        ...availableGain(),
        basisComplete: false,
        inputComplete: true,
        realizedGross: availableSigned("0.0000"),
        realizedNet: availableSigned("0.0000"),
        unrealizedGross: {
          kind: "unavailable",
          reason: "UNKNOWN_BASIS",
          blockingDates: [],
        },
      },
    });
    await router.navigate({
      to: "/analytics",
      search: { period: "oneYear" },
      replace: true,
    });
    await waitFor(() => {
      const unknown = screen.getByRole("table", { name: "Gain results" });
      expect(within(unknown).getAllByText("USD 0.0000").length).toBeGreaterThan(
        0,
      );
      expect(
        within(unknown).getByText(
          "This amount is unavailable because the lot has unknown cost basis.",
        ),
      ).toBeInTheDocument();
    });

    vi.mocked(commands.getGainSummary).mockResolvedValue({
      status: "ok",
      data: {
        ...availableGain(),
        basisComplete: true,
        inputComplete: false,
        realizedGross: availableSigned("160.0000"),
        realizedNet: availableSigned("146.0000"),
        unrealizedGross: {
          kind: "unavailable",
          reason: "ANALYTICS_INPUT_INCOMPLETE",
          blockingDates: [],
        },
      },
    });
    await router.navigate({
      to: "/analytics",
      search: { period: "threeMonths" },
      replace: true,
    });
    await waitFor(() => {
      const incomplete = screen.getByRole("table", { name: "Gain results" });
      expect(
        within(incomplete).getByText(
          "This result is incomplete because a required quote or FX rate is missing.",
        ),
      ).toBeInTheDocument();
    });
  });

  it("can tab to scope and period controls", async () => {
    const user = userEvent.setup();
    mockAnalyticsCatalog();
    mockAvailableAnalytics();
    await openAnalytics();
    await screen.findByRole("heading", { name: "Analytics" });
    const household = screen.getByRole("radio", { name: "Household" });
    household.focus();
    expect(household).toHaveFocus();
    await user.tab();
    expect(screen.getByRole("radio", { name: "1 month" })).toHaveFocus();
  });
});

async function openAnalytics() {
  await renderReadyApp();
  await router.navigate({ to: "/analytics", search: {}, replace: true });
  await screen.findByRole("heading", { name: "Analytics" });
}

function mockAnalyticsCatalog() {
  vi.mocked(commands.listAccounts).mockResolvedValue({
    status: "ok",
    data: [accountRecord("a-1", "Brokerage")],
  });
  vi.mocked(commands.listInstruments).mockResolvedValue({
    status: "ok",
    data: [instrumentRecord("ins-qqq", "QQQ", "USD")],
  });
}

function mockAvailableAnalytics() {
  vi.mocked(commands.getAnalyticsStatus).mockResolvedValue({
    status: "ok",
    data: availableStatus(),
  });
  vi.mocked(commands.getPerformanceSummary).mockResolvedValue({
    status: "ok",
    data: availablePerformance(),
  });
  vi.mocked(commands.getGainSummary).mockResolvedValue({
    status: "ok",
    data: availableGain(),
  });
  vi.mocked(commands.getNetWorthAttribution).mockResolvedValue({
    status: "ok",
    data: availableAttribution(),
  });
  vi.mocked(commands.listHoldingLots).mockResolvedValue({
    status: "ok",
    data: { items: [declaredLot(), unknownLot()], nextCursor: null, hasMore: false },
  });
  vi.mocked(commands.listUnknownBasisLots).mockResolvedValue({
    status: "ok",
    data: { items: [unknownLot()], nextCursor: null, hasMore: false },
  });
  vi.mocked(commands.listCostBasisDeclarations).mockResolvedValue({
    status: "ok",
    data: { items: [], nextCursor: null, hasMore: false },
  });
}

function availableStatus(): AnalyticsStatusDto {
  return {
    usableHistory: {
      kind: "available",
      startLocalDate: "2026-01-02",
      endLocalDate: "2026-01-05",
    },
    earliestCompleteSnapshotOn: { kind: "available", value: "2026-01-02" },
    blockingDates: ["2026-01-03", "2026-01-04"],
    unknownBasisLotCount: 1,
    unknownBasisValue: {
      kind: "available",
      value: { amount: "14490.0000", currency: "CNY" },
    },
  };
}

function availablePerformance(): PerformanceSummaryDto {
  return {
    twr: {
      kind: "available",
      method: "twr",
      flowAssumption: "startOfDay",
      cumulative: "0.040400",
      annualized: null,
      skippedDays: 0,
      linkedDays: 2,
    },
    xirr: {
      kind: "available",
      method: "xirr",
      annualRate: "0.100000",
    },
  };
}

function availableSigned(amount: string, currency = "USD"): {
  kind: "available";
  value: SignedMoneyDto;
} {
  return { kind: "available", value: { amount, currency } };
}

function availableGain(): GainSummaryIpcDto {
  return {
    realizedGross: availableSigned("160.0000"),
    realizedNet: availableSigned("146.0000"),
    allocatedFees: availableSigned("14.0000"),
    unrealizedGross: availableSigned("80.0000"),
    unexplainedDisposal: availableSigned("0.0000"),
    basisComplete: false,
    inputComplete: true,
    decompositionComplete: true,
    unknownBasisQuantity: "3",
    unknownBasisValue: {
      kind: "available",
      value: { amount: "14490.0000", currency: "CNY" },
    },
    instrumentMovement: availableSigned("650.0000", "CNY"),
    currencyMovement: availableSigned("120.0000", "CNY"),
    unrealizedAsOf: "currentSnapshot",
    income: [
      {
        incomeKind: "dividend",
        attributedInstrumentId: "ins-qqq",
        amount: { amount: "12.0000", currency: "USD" },
      },
    ],
    fees: [
      {
        feeKind: "tradeCommission",
        attributedInstrumentId: "ins-qqq",
        amount: { amount: "17.0000", currency: "USD" },
      },
    ],
  };
}

function availableAttribution(): NetWorthAttributionIpcDto {
  return {
    kind: "available",
    value: {
      startOn: "2026-01-02",
      endOn: "2026-01-05",
      startNetWorth: { amount: "100000.0000", currency: "CNY" },
      endNetWorth: { amount: "118000.0000", currency: "CNY" },
      delta: { amount: "18000.0000", currency: "CNY" },
      externalContributions: { amount: "10000.0000", currency: "CNY" },
      externalWithdrawals: { amount: "0.0000", currency: "CNY" },
      instrumentMovement: { amount: "5900.0000", currency: "CNY" },
      currencyMovement: { amount: "1800.0000", currency: "CNY" },
      income: { amount: "500.0000", currency: "CNY" },
      fees: { amount: "-200.0000", currency: "CNY" },
      debtPrincipalMovement: { amount: "0.0000", currency: "CNY" },
      conversionSpread: { amount: "-100.0000", currency: "CNY" },
      unexplained: { amount: "100.0000", currency: "CNY" },
      unknownBasisFlow: { amount: "0.0000", currency: "CNY" },
      basisComplete: false,
      methodNote: "unused",
    },
  };
}

function unknownLot(): HoldingLotDto {
  return {
    lotRef: { sourceKind: "originHolding", sourceId: "origin-qqq" },
    accountId: "a-1",
    instrumentId: "ins-qqq",
    acquiredAt: TIMESTAMP,
    quantityRemaining: "3",
    originalQuantity: "3",
    cost: {
      kind: "unavailable",
      reason: "UNKNOWN_BASIS",
      blockingDates: [],
    },
    basis: "unknown",
    isDeclared: false,
    currentValue: {
      kind: "available",
      value: { amount: "2100.0000", currency: "USD" },
    },
    unrealizedGross: {
      kind: "unavailable",
      reason: "UNKNOWN_BASIS",
      blockingDates: [],
    },
  };
}

function declaredLot(): HoldingLotDto {
  return {
    lotRef: { sourceKind: "acquisition", sourceId: "leg-voo" },
    accountId: "a-1",
    instrumentId: "ins-qqq",
    acquiredAt: TIMESTAMP,
    quantityRemaining: "2",
    originalQuantity: "4",
    cost: {
      kind: "available",
      value: { amount: "240.0000", currency: "USD" },
    },
    basis: "known",
    isDeclared: true,
    currentValue: {
      kind: "available",
      value: { amount: "320.0000", currency: "USD" },
    },
    unrealizedGross: availableSigned("80.0000"),
  };
}

function instrumentRecord(
  id: string,
  name: string,
  quoteCurrency: string,
): InstrumentRecordDto {
  return {
    id,
    name,
    symbol: name,
    instrumentType: "etf",
    quoteCurrency,
    marketCode: null,
    countryCode: null,
    isin: null,
    providerKey: null,
    providerSymbol: null,
    quotePreference: "manual",
    note: null,
    logoAssetId: null,
    sortOrder: 0,
    createdAt: TIMESTAMP,
    updatedAt: TIMESTAMP,
    archivedAt: null,
  };
}
