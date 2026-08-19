import { screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { commands } from "@/generated/tauri-bindings";
import {
  accountRecord,
  activityDetail,
  commandError,
  deferred,
  emptyTimelinePage,
  renderReadyApp,
  resetApp,
} from "@/test/app-harness";

describe("account timeline", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("renders backend order and classifications without inference", async () => {
    const user = userEvent.setup();
    mockAccountDetail();
    vi.mocked(commands.getAccountTimeline).mockResolvedValue({
      status: "ok",
      data: {
        items: [
          {
            kind: "origin",
            id: "origin-1",
            occurredAt: "2026-08-01T00:00:00.000Z",
            createdAt: "2026-08-01T00:00:00.000Z",
            localDate: "2026-08-01",
            label: "opening_state",
          },
          {
            kind: "activity",
            occurredAt: "2026-08-17T00:00:00.000Z",
            createdAt: "2026-08-17T00:00:00.000Z",
            activity: activityDetail("act-1", {
              reversed: true,
              classification: "external_inflow",
            }),
          },
          {
            kind: "observation",
            id: "obs-1",
            occurredAt: "2026-07-01T00:00:00.000Z",
            createdAt: "2026-07-01T00:00:00.000Z",
            componentKind: "account_value",
            amount: "5000",
            currency: "SGD",
          },
          {
            kind: "account_state",
            id: "st-1",
            occurredAt: "2026-08-18T00:00:00.000Z",
            createdAt: "2026-08-18T00:00:00.000Z",
            archived: true,
            primaryCategory: "cash_equivalent",
          },
        ],
        nextCursor: null,
        hasMore: false,
      },
    });
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Accounts" }));
    await user.click(await screen.findByRole("link", { name: "Open" }));
    expect(
      await screen.findByRole("heading", { name: "Timeline" }),
    ).toBeInTheDocument();
    expect(
      await screen.findByText("History starts from existing state."),
    ).toBeInTheDocument();
    const timeline = screen
      .getByRole("heading", { name: "Timeline" })
      .closest("section")!;
    expect(timeline.textContent).toMatch(
      /History origin[\s\S]*History starts from existing state[\s\S]*Deposit[\s\S]*Reversed[\s\S]*Legacy observation[\s\S]*Account archived/i,
    );
    expect(screen.getByText("SGD 5,000")).toBeInTheDocument();
    expect(screen.getByText(/External inflow/)).toBeInTheDocument();
  });

  it("shows loading, empty, and error timeline states", async () => {
    const user = userEvent.setup();
    mockAccountDetail();
    const pending = deferred<{
      status: "ok";
      data: ReturnType<typeof emptyTimelinePage>;
    }>();
    vi.mocked(commands.getAccountTimeline).mockReturnValue(pending.promise);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Accounts" }));
    await user.click(await screen.findByRole("link", { name: "Open" }));
    expect(await screen.findAllByRole("status")).toEqual(
      expect.arrayContaining([expect.objectContaining({ textContent: "Loading…" })]),
    );
    pending.resolve({ status: "ok", data: emptyTimelinePage() });
    expect(await screen.findByText("No timeline items yet.")).toBeInTheDocument();
  });

  it("exposes timeline errors with role=alert", async () => {
    const user = userEvent.setup();
    mockAccountDetail();
    vi.mocked(commands.getAccountTimeline).mockResolvedValue({
      status: "error",
      error: commandError("DATABASE_UNAVAILABLE", "The database is unavailable."),
    });
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Accounts" }));
    await user.click(await screen.findByRole("link", { name: "Open" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The database is unavailable.",
    );
  });
});

function mockAccountDetail() {
  const account = accountRecord("a-1", "DBS Savings");
  vi.mocked(commands.listAccounts).mockResolvedValue({
    status: "ok",
    data: [account],
  });
  vi.mocked(commands.getAccount).mockResolvedValue({
    status: "ok",
    data: account,
  });
}
