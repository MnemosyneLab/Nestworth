import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { router } from "@/app/router";
import type { GainSummaryIpcDto, PortfolioDto } from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import {
  commandError,
  deferred,
  emptyGainSummary,
  emptyPortfolio,
  renderReadyApp,
  resetApp,
} from "@/test/app-harness";

describe("investments page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("renders portfolio totals from the backend DTO", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.getPortfolio).mockResolvedValue({
      status: "ok",
      data: {
        ...emptyPortfolio(),
        total: { amount: "62190", currency: "CNY" },
        isComplete: true,
        coverageBps: 10_000,
        positions: [
          {
            holdingId: "h-1",
            accountId: "a-1",
            instrumentId: "ins-1",
            instrumentName: "QQQ",
            instrumentSymbol: "QQQ",
            instrumentType: "etf",
            countryCode: "US",
            quantity: "3",
            native: { amount: "2100", currency: "USD" },
            base: { amount: "14490", currency: "CNY" },
            complete: true,
            freshness: "manual",
            quotedAt: "2026-08-17T00:00:00.000Z",
            sourceKind: "manual",
            missingReason: null,
          },
        ],
        cash: [{ amount: "5000", currency: "SGD" }],
        byCurrency: [
          {
            key: "USD",
            name: "USD",
            amount: { amount: "14490", currency: "CNY" },
            shareBps: 2330,
          },
        ],
      },
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Investments" }));
    expect(await screen.findByText("CNY 62,190")).toBeInTheDocument();
    expect(screen.getByText("All required quotes are available.")).toBeInTheDocument();
    expect(
      screen.queryByRole("button", { name: "Refresh quotes" }),
    ).not.toBeInTheDocument();
    expect(screen.getByText("QQQ")).toBeInTheDocument();
    expect(screen.getAllByText("CNY 14,490").length).toBeGreaterThan(0);
    expect(screen.getByText("SGD 5,000")).toBeInTheDocument();
  });

  it("shows incomplete diagnostics instead of treating missing quotes as zero", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.getPortfolio).mockResolvedValue({
      status: "ok",
      data: {
        ...emptyPortfolio(),
        total: { amount: "0", currency: "CNY" },
        isComplete: false,
        coverageBps: 0,
        unvaluedItems: [
          {
            kind: "holding",
            id: "h-1",
            name: "QQQ",
            reason: "instrument_quote",
          },
        ],
        positions: [
          {
            holdingId: "h-1",
            accountId: "a-1",
            instrumentId: "ins-1",
            instrumentName: "QQQ",
            instrumentSymbol: "QQQ",
            instrumentType: "etf",
            countryCode: "US",
            quantity: "3",
            native: null,
            base: null,
            complete: false,
            freshness: "unavailable",
            quotedAt: null,
            sourceKind: "manual",
            missingReason: "instrument_quote",
          },
        ],
      },
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Investments" }));
    expect(await screen.findByText(/Incomplete/)).toBeInTheDocument();
    expect(screen.getByText("QQQ — Missing instrument quote")).toBeInTheDocument();
    expect(screen.getByText("Unavailable")).toBeInTheDocument();
  });

  it("explains and submits FX rates in the persisted direction", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.getPortfolio).mockResolvedValue({
      status: "ok",
      data: {
        ...emptyPortfolio(),
        requiredFx: [
          {
            currencyA: "CNY",
            currencyB: "SGD",
            quotePreference: "manual",
            selectedQuote: {
              id: "fx-1",
              baseCurrency: "SGD",
              quoteCurrency: "CNY",
              rate: "5.3",
              sourceKind: "manual",
              sourceKey: "manual",
              delayed: false,
              quotedAt: "2026-08-18T00:00:00.000Z",
              createdAt: "2026-08-18T00:00:00.000Z",
            },
            selectedRate: "5.3",
          },
        ],
      },
    });
    vi.mocked(commands.listFxQuotes).mockResolvedValue({
      status: "ok",
      data: [
        {
          id: "fx-1",
          baseCurrency: "SGD",
          quoteCurrency: "CNY",
          rate: "5.3",
          sourceKind: "manual",
          sourceKey: "manual",
          delayed: false,
          quotedAt: "2026-08-18T00:00:00.000Z",
          createdAt: "2026-08-18T00:00:00.000Z",
        },
      ],
    });
    vi.mocked(commands.appendManualFxQuote).mockResolvedValue({
      status: "ok",
      data: {
        id: "fx-2",
        baseCurrency: "SGD",
        quoteCurrency: "CNY",
        rate: "5.3",
        sourceKind: "manual",
        sourceKey: "manual",
        delayed: false,
        quotedAt: "2026-08-18T00:00:00.000Z",
        createdAt: "2026-08-18T00:00:00.000Z",
      },
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Investments" }));
    expect(await screen.findByText("1 SGD = [rate] CNY")).toBeInTheDocument();
    expect(screen.getByText(/Selected quote: 1 SGD = 5\.3 CNY/)).toBeInTheDocument();
    expect(
      screen.getByText(/Updating this rate revalues net worth/),
    ).toBeInTheDocument();
    await user.type(screen.getByLabelText("Rate for 1 SGD in CNY"), "5.3");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(commands.appendManualFxQuote).toHaveBeenCalledWith({
      baseCurrency: "SGD",
      quoteCurrency: "CNY",
      rate: "5.3",
      quotedAt: null,
    });
  });

  it("shows unknown-basis positions as unknown rather than zero", async () => {
    const user = userEvent.setup();
    mockPortfolio(qqqPortfolio());
    vi.mocked(commands.listHoldingGainSummaries).mockResolvedValue({
      status: "ok",
      data: unknownBasisHoldingGains(),
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Investments" }));
    expect(await screen.findByText("CNY 62,190")).toBeInTheDocument();
    expect(screen.getAllByText("Unknown").length).toBeGreaterThan(0);
    expect(
      screen.getByText(
        "Unknown-basis positions are excluded from gain totals. Their quantity and value are reported separately rather than treated as zero cost.",
      ),
    ).toHaveAttribute("role", "status");
    expect(screen.queryByText("USD 0")).not.toBeInTheDocument();
    expect(screen.queryByText("CNY 0")).not.toBeInTheDocument();
    expect(commands.listHoldingGainSummaries).toHaveBeenCalledWith({
      period: { kind: "all" },
    });
    expect(commands.getGainSummary).not.toHaveBeenCalled();
  });

  it("links a position into instrument-scoped analytics", async () => {
    const user = userEvent.setup();
    mockPortfolio(qqqPortfolio());
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Investments" }));
    await user.click(
      await screen.findByRole("link", { name: "View analytics for QQQ" }),
    );
    await waitFor(() => {
      expect(router.state.location.pathname).toBe("/analytics");
      expect(router.state.location.search).toEqual(
        expect.objectContaining({
          scope: "instrument",
          instrumentId: "ins-1",
          period: "all",
        }),
      );
    });
    await router.navigate({ to: "/overview", replace: true });
  });

  it("shows loading, error, and incomplete gain states accessibly", async () => {
    const pending = deferred<{
      status: "ok";
      data: { items: Array<{ accountId: string; instrumentId: string; gain: GainSummaryIpcDto }> };
    }>();
    mockPortfolio(qqqPortfolio());
    vi.mocked(commands.listHoldingGainSummaries).mockReturnValue(pending.promise);
    await renderReadyApp();
    await router.navigate({ to: "/investments" });
    expect(await screen.findByRole("heading", { name: "Portfolio" })).toBeInTheDocument();
    expect(await screen.findByText("QQQ")).toBeInTheDocument();
    expect(screen.getByRole("status")).toHaveTextContent("Loading…");
    pending.resolve({
      status: "ok",
      data: {
        items: [
          {
            accountId: "a-1",
            instrumentId: "ins-1",
            gain: {
              ...emptyGainSummary(),
              inputComplete: false,
              unrealizedGross: {
                kind: "unavailable",
                reason: "ANALYTICS_INPUT_INCOMPLETE",
                blockingDates: ["2026-08-17"],
              },
            },
          },
        ],
      },
    });
    expect(await screen.findByText("This result is incomplete.")).toHaveAttribute(
      "role",
      "status",
    );
    expect(
      screen.getByText(
        "This result is incomplete because a required quote or FX rate is missing.",
      ),
    ).toBeInTheDocument();
  });

  it("shows a gain command error accessibly", async () => {
    const user = userEvent.setup();
    mockPortfolio(qqqPortfolio());
    vi.mocked(commands.listHoldingGainSummaries).mockResolvedValue({
      status: "error",
      error: commandError("DATABASE_UNAVAILABLE", "The database is unavailable."),
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Investments" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The database is unavailable.",
    );
  });

  it("does not invalidate valuation when showing analytics links", async () => {
    const user = userEvent.setup();
    mockPortfolio(qqqPortfolio());
    await renderReadyApp();
    const overviewCalls = vi.mocked(commands.getOverview).mock.calls.length;
    const trendCalls = vi.mocked(commands.getNetWorthTrend).mock.calls.length;
    const historyCalls = vi.mocked(commands.getHistoryStatus).mock.calls.length;
    const activityCalls = vi.mocked(commands.listActivities).mock.calls.length;
    await user.click(await screen.findByRole("link", { name: "Investments" }));
    expect(await screen.findByText("CNY 62,190")).toBeInTheDocument();
    await screen.findByRole("link", { name: "View analytics for QQQ" });
    expect(vi.mocked(commands.getOverview).mock.calls.length).toBe(overviewCalls);
    expect(vi.mocked(commands.getNetWorthTrend).mock.calls.length).toBe(trendCalls);
    expect(vi.mocked(commands.getHistoryStatus).mock.calls.length).toBe(
      historyCalls,
    );
    expect(
      vi.mocked(commands.listActivities).mock.calls.length,
    ).toBe(activityCalls);
  });

  it("shows per-holding gain when the same instrument is held in two accounts", async () => {
    const user = userEvent.setup();
    const first = qqqPosition("h-1", "a-1", "3");
    const second = qqqPosition("h-2", "a-2", "1");
    mockPortfolio({
      ...qqqPortfolio(),
      positions: [first, second],
    });
    vi.mocked(commands.listHoldingGainSummaries).mockResolvedValue({
      status: "ok",
      data: {
        items: [
          {
            accountId: "a-1",
            instrumentId: "ins-1",
            gain: {
              ...emptyGainSummary(),
              basisComplete: true,
              inputComplete: true,
              unrealizedGross: {
                kind: "available",
                value: { amount: "120.0000", currency: "USD" },
              },
            },
          },
          {
            accountId: "a-2",
            instrumentId: "ins-1",
            gain: {
              ...emptyGainSummary(),
              basisComplete: true,
              inputComplete: true,
              unrealizedGross: {
                kind: "available",
                value: { amount: "40.0000", currency: "USD" },
              },
            },
          },
        ],
      },
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Investments" }));
    expect(await screen.findByText("USD 120.0000")).toBeInTheDocument();
    expect(screen.getByText("USD 40.0000")).toBeInTheDocument();
    expect(commands.listHoldingGainSummaries).toHaveBeenCalledTimes(1);
    expect(commands.getGainSummary).not.toHaveBeenCalled();
    expect(
      screen.getAllByRole("link", { name: "View analytics for QQQ" }),
    ).toHaveLength(2);
  });

  it("shows zero realized for a known holding and incomplete for missing quotes", async () => {
    const user = userEvent.setup();
    mockPortfolio(qqqPortfolio());
    vi.mocked(commands.listHoldingGainSummaries).mockResolvedValue({
      status: "ok",
      data: {
        items: [
          {
            accountId: "a-1",
            instrumentId: "ins-1",
            gain: {
              ...emptyGainSummary(),
              basisComplete: true,
              inputComplete: true,
              realizedNet: {
                kind: "available",
                value: { amount: "0.0000", currency: "USD" },
              },
              unrealizedGross: {
                kind: "available",
                value: { amount: "80.0000", currency: "USD" },
              },
            },
          },
        ],
      },
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Investments" }));
    expect(await screen.findByText("USD 80.0000")).toBeInTheDocument();
    expect(screen.queryByText("Unknown")).not.toBeInTheDocument();

    vi.mocked(commands.listHoldingGainSummaries).mockResolvedValue({
      status: "ok",
      data: {
        items: [
          {
            accountId: "a-1",
            instrumentId: "ins-1",
            gain: {
              ...emptyGainSummary(),
              basisComplete: true,
              inputComplete: false,
              realizedNet: {
                kind: "available",
                value: { amount: "50.0000", currency: "USD" },
              },
              unrealizedGross: {
                kind: "unavailable",
                reason: "ANALYTICS_INPUT_INCOMPLETE",
                blockingDates: [],
              },
            },
          },
        ],
      },
    });
    await user.click(await screen.findByRole("link", { name: "Overview" }));
    await user.click(await screen.findByRole("link", { name: "Investments" }));
    expect(
      await screen.findByText(
        "This result is incomplete because a required quote or FX rate is missing.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText("This result is incomplete.")).toHaveAttribute(
      "role",
      "status",
    );
    expect(screen.queryByText("Unknown")).not.toBeInTheDocument();
  });
});

function mockPortfolio(portfolio: PortfolioDto) {
  vi.mocked(commands.getPortfolio).mockResolvedValue({
    status: "ok",
    data: portfolio,
  });
}

function qqqPortfolio(): PortfolioDto {
  return {
    ...emptyPortfolio(),
    total: { amount: "62190", currency: "CNY" },
    isComplete: true,
    coverageBps: 10_000,
    positions: [qqqPosition("h-1", "a-1", "3")],
    cash: [{ amount: "5000", currency: "SGD" }],
    byCurrency: [
      {
        key: "USD",
        name: "USD",
        amount: { amount: "14490", currency: "CNY" },
        shareBps: 2330,
      },
    ],
  };
}

function qqqPosition(
  holdingId: string,
  accountId: string,
  quantity: string,
): PortfolioDto["positions"][number] {
  return {
    holdingId,
    accountId,
    instrumentId: "ins-1",
    instrumentName: "QQQ",
    instrumentSymbol: "QQQ",
    instrumentType: "etf",
    countryCode: "US",
    quantity,
    native: { amount: "2100", currency: "USD" },
    base: { amount: "14490", currency: "CNY" },
    complete: true,
    freshness: "manual",
    quotedAt: "2026-08-17T00:00:00.000Z",
    sourceKind: "manual",
    missingReason: null,
  };
}

function unknownBasisGain(): GainSummaryIpcDto {
  return {
    ...emptyGainSummary(),
    basisComplete: false,
    inputComplete: true,
    unknownBasisQuantity: "3",
    unrealizedGross: {
      kind: "unavailable",
      reason: "UNKNOWN_BASIS",
      blockingDates: [],
    },
  };
}

function unknownBasisHoldingGains() {
  return {
    items: [
      {
        accountId: "a-1",
        instrumentId: "ins-1",
        gain: unknownBasisGain(),
      },
    ],
  };
}
