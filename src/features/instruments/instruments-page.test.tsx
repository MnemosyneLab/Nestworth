import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type { InstrumentRecordDto } from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import { renderReadyApp, resetApp } from "@/test/app-harness";

describe("instruments page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("creates an instrument from the instruments page", async () => {
    const user = userEvent.setup();
    const instruments: InstrumentRecordDto[] = [];
    vi.mocked(commands.listInstruments).mockImplementation(async (input) => ({
      status: "ok",
      data: input.includeArchived
        ? [...instruments]
        : instruments.filter((item) => item.archivedAt === null),
    }));
    vi.mocked(commands.createInstrument).mockImplementation(async (input) => {
      const created: InstrumentRecordDto = {
        id: "ins-1",
        name: input.name,
        symbol: input.symbol,
        instrumentType: input.instrumentType,
        quoteCurrency: input.quoteCurrency,
        marketCode: input.marketCode,
        countryCode: input.countryCode,
        isin: input.isin,
        providerKey: input.providerKey,
        providerSymbol: input.providerSymbol,
        quotePreference: input.quotePreference ?? "manual",
        note: input.note,
        logoAssetId: null,
        sortOrder: 0,
        createdAt: "2026-08-17T00:00:00.000Z",
        updatedAt: "2026-08-17T00:00:00.000Z",
        archivedAt: null,
      };
      instruments.push(created);
      return { status: "ok", data: created };
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Instruments" }));
    expect(
      await screen.findByText("Add an instrument to record holdings."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add instrument" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    expect(screen.getByLabelText("Currency").tagName).toBe("SELECT");
    expect(screen.queryByRole("textbox", { name: "Currency" })).not.toBeInTheDocument();
    await user.type(screen.getByLabelText("Name"), "QQQ");
    await user.type(screen.getByLabelText("Symbol"), "QQQ");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByRole("heading", { name: "QQQ" })).toBeInTheDocument();
    expect(commands.createInstrument).toHaveBeenCalledWith({
      name: "QQQ",
      symbol: "QQQ",
      instrumentType: "etf",
      quoteCurrency: "USD",
      marketCode: null,
      countryCode: null,
      isin: null,
      providerKey: null,
      providerSymbol: null,
      quotePreference: "manual",
      note: null,
    });
  });

  it("does not expose an unusable provider choice in production", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.listInstruments).mockResolvedValue({ status: "ok", data: [] });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Instruments" }));
    await user.click(screen.getByRole("button", { name: "Add instrument" }));
    expect(screen.queryByLabelText("Quote preference")).not.toBeInTheDocument();
    expect(screen.queryByRole("option", { name: "Provider" })).not.toBeInTheDocument();
  });

  it("saves a complete Yahoo provider binding only after capability discovery", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.listInstruments).mockResolvedValue({ status: "ok", data: [] });
    vi.mocked(commands.getMarketDataCapabilities).mockResolvedValue({
      status: "ok",
      data: {
        defaultProviderId: "yahoo_finance",
        providers: [
          {
            providerId: "yahoo_finance",
            providerName: "Yahoo Finance",
            latestInstrument: true,
            latestFx: true,
            dailyHistory: true,
            instrumentSearch: false,
          },
        ],
      },
    });
    vi.mocked(commands.createInstrument).mockResolvedValue({
      status: "ok",
      data: {
        id: "ins-provider",
        name: "QQQ",
        symbol: "QQQ",
        instrumentType: "etf",
        quoteCurrency: "USD",
        marketCode: null,
        countryCode: null,
        isin: null,
        providerKey: "yahoo_finance",
        providerSymbol: "QQQ",
        quotePreference: "provider",
        note: null,
        logoAssetId: null,
        sortOrder: 0,
        createdAt: "2026-08-17T00:00:00.000Z",
        updatedAt: "2026-08-17T00:00:00.000Z",
        archivedAt: null,
      },
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Instruments" }));
    await user.click(screen.getByRole("button", { name: "Add instrument" }));
    await user.type(screen.getByLabelText("Name"), "QQQ");
    await user.selectOptions(screen.getByLabelText("Quote preference"), "provider");
    await user.type(screen.getByLabelText("Provider symbol"), "QQQ");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(commands.createInstrument).toHaveBeenCalledWith(
      expect.objectContaining({
        providerKey: "yahoo_finance",
        providerSymbol: "QQQ",
        quotePreference: "provider",
      }),
    );
  });
});
