import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { commands } from "@/generated/tauri-bindings";
import { emptyPortfolio, renderReadyApp, resetApp } from "@/test/app-harness";

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
    await user.type(screen.getByLabelText("Rate for 1 SGD in CNY"), "5.3");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(commands.appendManualFxQuote).toHaveBeenCalledWith({
      baseCurrency: "SGD",
      quoteCurrency: "CNY",
      rate: "5.3",
      quotedAt: null,
    });
  });
});
