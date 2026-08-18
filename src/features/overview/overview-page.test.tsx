import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  HistoryStatusDto,
  NetWorthTrendDto,
  NetWorthTrendPointDto,
  OverviewDto,
  RebuildHistorySnapshotsResultDto,
} from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import {
  commandError,
  deferred,
  emptyHistoryStatus,
  emptyNetWorthTrend,
  emptyOverview,
  renderReadyApp,
  resetApp,
} from "@/test/app-harness";

describe("overview page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("shows the empty balance sheet instead of zero totals", async () => {
    await renderReadyApp();
    expect(
      await screen.findByText("Your household balance sheet is empty."),
    ).toBeInTheDocument();
    expect(
      screen.getByText("Add your first account to start tracking your net worth."),
    ).toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Add account" })).toBeInTheDocument();
    expect(screen.queryByText("CNY 0")).not.toBeInTheDocument();
  });

  it("renders golden totals from the overview command", async () => {
    mockPopulatedOverview();
    await renderReadyApp();
    expect(await screen.findByText("CNY 3,110,000")).toBeInTheDocument();
    expect(screen.getByText("CNY 4,110,000")).toBeInTheDocument();
    expect(screen.getByText("CNY 1,000,000")).toBeInTheDocument();
    expect(screen.getByText("Cash equivalent")).toBeInTheDocument();
    expect(screen.getByText("CNY 110,000")).toBeInTheDocument();
    const byMember = screen
      .getByRole("heading", { name: "By member" })
      .closest("section");
    expect(byMember).not.toBeNull();
    expect(within(byMember!).getByText("Walt")).toBeInTheDocument();
    expect(within(byMember!).getByText("CNY 1,600,000")).toBeInTheDocument();
    expect(within(byMember!).getByText("Spouse")).toBeInTheDocument();
    expect(within(byMember!).getByText("CNY 1,510,000")).toBeInTheDocument();
    expect(
      screen.queryByText("Your household balance sheet is empty."),
    ).not.toBeInTheDocument();
  });

  it("shows loading status while overview is pending", async () => {
    const pending = deferred<{
      status: "ok";
      data: ReturnType<typeof emptyOverview>;
    }>();
    vi.mocked(commands.getOverview).mockReturnValue(pending.promise);
    await renderReadyApp();
    expect(await screen.findByRole("status")).toHaveTextContent("Loading…");
    pending.resolve({ status: "ok", data: emptyOverview() });
    expect(
      await screen.findByText("Your household balance sheet is empty."),
    ).toBeInTheDocument();
  });

  it("shows a command error when overview cannot load", async () => {
    vi.mocked(commands.getOverview).mockResolvedValue({
      status: "error",
      error: commandError("DATABASE_UNAVAILABLE", "The database is unavailable."),
    });
    await renderReadyApp();
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The database is unavailable.",
    );
  });

  it("charts and lists exact Money strings from the trend DTO", async () => {
    const trend = trendDto({
      points: [
        historicalPoint({
          date: "2026-08-14",
          netWorth: money("1234.5678"),
        }),
        historicalPoint({
          date: "2026-08-15",
          netWorth: money("9876.5432"),
        }),
      ],
      current: livePoint({ netWorth: money("3110000") }),
    });
    mockPopulatedOverview();
    mockTrend(trend);
    await renderReadyApp();
    const table = await screen.findByRole("table", { name: "Net-worth history" });
    expect(within(table).getByText("CNY 1,234.5678")).toBeInTheDocument();
    expect(within(table).getByText("CNY 9,876.5432")).toBeInTheDocument();
    expect(within(table).getByText("CNY 3,110,000")).toBeInTheDocument();
    expect(document.querySelector("svg[aria-hidden='true']")).not.toBeNull();
    expect(screen.queryByText("return")).not.toBeInTheDocument();
    expect(screen.queryByText("gain")).not.toBeInTheDocument();
    expect(screen.queryByText("benchmark")).not.toBeInTheDocument();
    expect(screen.queryByText("attribution")).not.toBeInTheDocument();
  });

  it("keeps displayed financial values when the chart scales coordinates", async () => {
    const trend = trendDto({
      points: [
        historicalPoint({
          date: "2026-08-14",
          netWorth: money("0.0001"),
        }),
        historicalPoint({
          date: "2026-08-16",
          netWorth: money("100000000000"),
        }),
      ],
      current: livePoint({ netWorth: money("3110000.25") }),
    });
    mockPopulatedOverview();
    mockTrend(trend);
    await renderReadyApp();
    const table = await screen.findByRole("table", { name: "Net-worth history" });
    expect(within(table).getByText("CNY 0.0001")).toBeInTheDocument();
    expect(within(table).getByText("CNY 100,000,000,000")).toBeInTheDocument();
    expect(within(table).getByText("CNY 3,110,000.25")).toBeInTheDocument();
    expect(screen.queryByText("CNY 0")).not.toBeInTheDocument();
  });

  it("distinguishes incomplete points and exposes missing counts", async () => {
    const trend = trendDto({
      points: [
        historicalPoint({
          date: "2026-08-14",
          netWorth: money("1000"),
        }),
        historicalPoint({
          date: "2026-08-15",
          netWorth: money("2000"),
        }),
        historicalPoint({
          date: "2026-08-16",
          netWorth: money("1500"),
          isComplete: false,
          missingCount: 3,
          valuedComponentCount: 1,
          totalComponentCount: 4,
        }),
      ],
      current: livePoint({ netWorth: money("3110000") }),
    });
    mockPopulatedOverview();
    mockTrend(trend);
    await renderReadyApp();
    const table = await screen.findByRole("table", { name: "Net-worth history" });
    const incompleteRow = within(table).getByRole("row", {
      name: "2026-08-16, CNY 1,500, CNY, Incomplete, 3 missing",
    });
    expect(incompleteRow).toHaveAttribute("data-trend-state", "incomplete");
    expect(within(incompleteRow).getByText("3")).toBeInTheDocument();
    expect(
      within(incompleteRow).getByText("Incomplete, 3 missing"),
    ).toBeInTheDocument();
    const marker = document.querySelector('circle[data-trend-state="incomplete"]');
    expect(marker).not.toBeNull();
    expect(marker).toHaveAttribute("fill", "none");
    expect(marker).toHaveAttribute("stroke-dasharray");
    expect(document.querySelectorAll('path[data-segment="trusted"]')).toHaveLength(1);
    expect(document.querySelectorAll('path[data-segment="distinct"]')).toHaveLength(2);
  });

  it("explains pre-origin ranges instead of displaying zero history", async () => {
    mockPopulatedOverview();
    mockTrend(
      trendDto({
        originLocalDate: "2026-08-17",
        points: [],
        current: livePoint({ netWorth: money("3110000") }),
      }),
    );
    await renderReadyApp();
    expect(
      await screen.findByText(
        "History starts on 2026-08-17. Dates before this origin are unavailable.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText("No closed-day snapshots are available in this range."),
    ).toBeInTheDocument();
    const table = screen.getByRole("table", { name: "Net-worth history" });
    expect(within(table).queryByText("2026-08-16")).not.toBeInTheDocument();
    expect(within(table).queryByText("CNY 0")).not.toBeInTheDocument();
    expect(within(table).getByText("Current")).toBeInTheDocument();
  });

  it("prompts a local rebuild when history is dirty and does not refresh providers", async () => {
    const user = userEvent.setup();
    mockPopulatedOverview();
    mockTrend(
      trendDto({
        dirtyFrom: "2026-08-01",
        points: [
          historicalPoint({
            date: "2026-08-16",
            netWorth: money("3110000"),
          }),
        ],
        current: livePoint({ netWorth: money("3110000") }),
      }),
    );
    mockHistoryStatus({
      dirtyFrom: "2026-08-01",
      lastCompletedOn: null,
      rebuildStatus: "idle",
    });
    await renderReadyApp();
    expect(
      await screen.findByText(
        "Snapshots from 2026-08-01 need a local rebuild before this range is current.",
      ),
    ).toBeInTheDocument();
    expect(
      screen.getByText(
        "Rebuild runs on this device and does not refresh quotes or contact providers.",
      ),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Rebuild snapshots" }));
    await waitFor(() => {
      expect(commands.rebuildHistorySnapshots).toHaveBeenCalledWith({});
    });
    expect(commands.refreshAll).not.toHaveBeenCalled();
    expect(commands.refreshInstrument).not.toHaveBeenCalled();
    expect(commands.refreshRequiredFx).not.toHaveBeenCalled();
  });

  it("keeps completed revisions and dirty state after rebuild cancellation", async () => {
    const user = userEvent.setup();
    const pending = deferred<{
      status: "ok";
      data: RebuildHistorySnapshotsResultDto;
    }>();
    mockPopulatedOverview();
    mockTrend(
      trendDto({
        dirtyFrom: "2026-08-01",
        points: [
          historicalPoint({
            date: "2026-08-09",
            netWorth: money("2800000"),
          }),
        ],
        current: livePoint({ netWorth: money("3110000") }),
      }),
    );
    mockHistoryStatus({
      dirtyFrom: "2026-08-01",
      lastCompletedOn: "2026-08-09",
      rebuildStatus: "running",
    });
    vi.mocked(commands.rebuildHistorySnapshots).mockImplementation((input) => {
      if (input.cancel) {
        return Promise.resolve({
          status: "ok",
          data: {
            processedDays: 9,
            remaining: true,
            cancelled: true,
            dirtyFrom: "2026-08-10",
            lastCompletedOn: "2026-08-09",
            status: "cancelled",
          },
        });
      }
      return pending.promise;
    });
    await renderReadyApp();
    expect(
      await screen.findByRole("button", { name: "Cancel rebuild" }),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Cancel rebuild" }));
    expect(
      await screen.findByText(
        "Rebuild cancelled. Completed revisions are kept. Dirty from 2026-08-10.",
      ),
    ).toBeInTheDocument();
    expect(screen.getByText(/Last completed 2026-08-09/)).toBeInTheDocument();
    expect(commands.rebuildHistorySnapshots).toHaveBeenCalledWith({ cancel: true });
    expect(commands.refreshAll).not.toHaveBeenCalled();
  });

  it("shows the live current point using the trend net-worth DTO", async () => {
    const trend = trendDto({
      points: [
        historicalPoint({
          date: "2026-08-16",
          netWorth: money("3000000"),
        }),
      ],
      current: livePoint({ netWorth: money("3110000") }),
    });
    mockPopulatedOverview();
    mockTrend(trend);
    await renderReadyApp();
    const table = await screen.findByRole("table", { name: "Net-worth history" });
    const liveRow = within(table).getByRole("row", {
      name: "Current, CNY 3,110,000, CNY, Live",
    });
    expect(within(liveRow).getByText("CNY 3,110,000")).toBeInTheDocument();
    expect(liveRow).toHaveAttribute("data-trend-state", "live");
    expect(document.querySelector('circle[data-trend-state="live"]')).not.toBeNull();
  });

  it("exposes date, value, currency, and completeness to keyboard and screen readers", async () => {
    mockPopulatedOverview();
    mockTrend(
      trendDto({
        points: [
          historicalPoint({
            date: "2026-08-16",
            netWorth: money("1500"),
            isComplete: false,
            missingCount: 2,
          }),
        ],
        current: livePoint({ netWorth: money("3110000") }),
      }),
    );
    await renderReadyApp();
    const table = await screen.findByRole("table", { name: "Net-worth history" });
    expect(
      within(table).getByRole("columnheader", { name: "Date" }),
    ).toBeInTheDocument();
    expect(
      within(table).getByRole("columnheader", { name: "Value" }),
    ).toBeInTheDocument();
    expect(
      within(table).getByRole("columnheader", { name: "Currency" }),
    ).toBeInTheDocument();
    expect(
      within(table).getByRole("columnheader", { name: "Completeness" }),
    ).toBeInTheDocument();
    const row = within(table).getByRole("row", {
      name: "2026-08-16, CNY 1,500, CNY, Incomplete, 2 missing",
    });
    expect(row).toHaveAccessibleName(
      "2026-08-16, CNY 1,500, CNY, Incomplete, 2 missing",
    );
    row.focus();
    expect(row).toHaveFocus();
    expect(screen.getByRole("radiogroup", { name: "Trend range" })).toBeInTheDocument();
  });

  it("renders empty and one-point histories without chart errors", async () => {
    mockPopulatedOverview();
    mockTrend(
      trendDto({
        points: [],
        current: livePoint({ netWorth: money("3110000") }),
      }),
    );
    await renderReadyApp();
    expect(
      await screen.findByRole("table", { name: "Net-worth history" }),
    ).toBeInTheDocument();
    expect(document.querySelectorAll("circle")).toHaveLength(1);
    expect(document.querySelector("path[data-segment]")).toBeNull();
  });

  it("renders a one-point history without chart errors", async () => {
    mockPopulatedOverview();
    mockTrend(
      trendDto({
        points: [
          historicalPoint({
            date: "2026-08-16",
            netWorth: money("3110000"),
          }),
        ],
        current: livePoint({ netWorth: money("3110000") }),
      }),
    );
    await renderReadyApp();
    const table = await screen.findByRole("table", { name: "Net-worth history" });
    expect(within(table).getByText("2026-08-16")).toBeInTheDocument();
    expect(document.querySelectorAll("circle")).toHaveLength(2);
  });

  it("requests each selected trend range from the backend", async () => {
    const user = userEvent.setup();
    mockPopulatedOverview();
    mockTrend(trendDto({ current: livePoint({ netWorth: money("3110000") }) }));
    await renderReadyApp();
    await screen.findByRole("table", { name: "Net-worth history" });
    expect(commands.getNetWorthTrend).toHaveBeenCalledWith({ range: "1m" });
    await user.click(screen.getByRole("radio", { name: "Three months" }));
    await waitFor(() => {
      expect(commands.getNetWorthTrend).toHaveBeenCalledWith({ range: "3m" });
    });
    await user.click(screen.getByRole("radio", { name: "One year" }));
    await waitFor(() => {
      expect(commands.getNetWorthTrend).toHaveBeenCalledWith({ range: "1y" });
    });
    await user.click(screen.getByRole("radio", { name: "All history" }));
    await waitFor(() => {
      expect(commands.getNetWorthTrend).toHaveBeenCalledWith({ range: "all" });
    });
  });
});

function money(amount: string, currency = "CNY") {
  return { amount, currency };
}

function historicalPoint(
  extras: Partial<NetWorthTrendPointDto> & { date: string },
): NetWorthTrendPointDto {
  return {
    asOf: `${extras.date}T16:00:00.000Z`,
    assets: money("0"),
    liabilities: money("0"),
    netWorth: money("0"),
    isComplete: true,
    isLive: false,
    coverageBps: 10_000,
    missingCount: 0,
    valuedComponentCount: 1,
    totalComponentCount: 1,
    ...extras,
  };
}

function livePoint(extras: Partial<NetWorthTrendPointDto> = {}): NetWorthTrendPointDto {
  return {
    date: null,
    asOf: "2026-08-18T14:00:00.000Z",
    assets: money("4110000"),
    liabilities: money("1000000"),
    netWorth: money("3110000"),
    isComplete: true,
    isLive: true,
    coverageBps: 10_000,
    missingCount: 0,
    valuedComponentCount: 4,
    totalComponentCount: 4,
    ...extras,
  };
}

function trendDto(extras: Partial<NetWorthTrendDto> = {}): NetWorthTrendDto {
  return {
    ...emptyNetWorthTrend(),
    range: "1m",
    ...extras,
  };
}

function mockTrend(trend: NetWorthTrendDto) {
  vi.mocked(commands.getNetWorthTrend).mockResolvedValue({
    status: "ok",
    data: trend,
  });
}

function mockHistoryStatus(extras: Partial<HistoryStatusDto>) {
  vi.mocked(commands.getHistoryStatus).mockResolvedValue({
    status: "ok",
    data: {
      ...emptyHistoryStatus(),
      ...extras,
    },
  });
}

function mockPopulatedOverview() {
  vi.mocked(commands.getOverview).mockResolvedValue({
    status: "ok",
    data: goldenOverview(),
  });
}

function goldenOverview(): OverviewDto {
  return {
    baseCurrency: "CNY",
    accountCount: 4,
    assets: { amount: "4110000", currency: "CNY" },
    liabilities: { amount: "1000000", currency: "CNY" },
    netWorth: { amount: "3110000", currency: "CNY" },
    byCategory: [
      {
        key: "cash_equivalent",
        id: null,
        name: null,
        amount: { amount: "110000", currency: "CNY" },
        shareBps: 268,
      },
      {
        key: "property",
        id: null,
        name: null,
        amount: { amount: "4000000", currency: "CNY" },
        shareBps: 9732,
      },
    ],
    byMember: [
      {
        key: "member",
        id: "m-1",
        name: "Walt",
        amount: { amount: "1600000", currency: "CNY" },
        shareBps: 5109,
      },
      {
        key: "member",
        id: "m-2",
        name: "Spouse",
        amount: { amount: "1510000", currency: "CNY" },
        shareBps: 4891,
      },
    ],
    byInstitution: [
      {
        key: "i-1",
        id: "i-1",
        name: "DBS",
        amount: { amount: "100000", currency: "CNY" },
        shareBps: 243,
      },
    ],
    byGroup: [],
    isComplete: true,
    unvaluedItems: [],
  };
}
