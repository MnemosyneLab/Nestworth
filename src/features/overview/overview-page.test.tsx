import { screen, within } from "@testing-library/react";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { commands } from "@/generated/tauri-bindings";
import {
  commandError,
  deferred,
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
    vi.mocked(commands.getOverview).mockResolvedValue({
      status: "ok",
      data: {
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
      },
    });
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
});
