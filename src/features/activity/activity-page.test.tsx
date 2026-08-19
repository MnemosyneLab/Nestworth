import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { router } from "@/app/router";
import type {
  ActivityPreviewDto,
  CreateActivityInput,
  HoldingRecordDto,
} from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import {
  accountRecord,
  activityDetail,
  commandError,
  confirmedHistoryOrigin,
  deferred,
  renderReadyApp,
  resetApp,
} from "@/test/app-harness";

const DATE = "2026-08-18";
const TIME = "09:30";

describe("activity page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it.each([
    {
      name: "deposit",
      kindLabel: "Deposit",
      fill: async (user: ReturnType<typeof userEvent.setup>, form: HTMLElement) => {
        await user.selectOptions(within(form).getByLabelText("Account"), "a-1");
        await user.type(within(form).getByLabelText("Amount"), "3000");
      },
      expected: {
        kind: "deposit",
        accountId: "a-1",
        component: "account_value",
        amount: "3000",
        currency: "CNY",
      },
    },
    {
      name: "transfer",
      kindLabel: "Transfer",
      fill: async (user: ReturnType<typeof userEvent.setup>, form: HTMLElement) => {
        await user.selectOptions(within(form).getByLabelText("Source account"), "a-1");
        await user.selectOptions(
          within(form).getByLabelText("Destination account"),
          "a-2",
        );
        await user.type(within(form).getByLabelText("Source amount"), "3000");
        await user.type(within(form).getByLabelText("Destination amount"), "3000");
      },
      expected: {
        kind: "transfer",
        sourceAccountId: "a-1",
        destinationAccountId: "a-2",
        sourceAmount: "3000",
        destinationAmount: "3000",
        sourceCurrency: "CNY",
        destinationCurrency: "CNY",
      },
    },
    {
      name: "buy",
      kindLabel: "Buy",
      fill: async (user: ReturnType<typeof userEvent.setup>, form: HTMLElement) => {
        await user.selectOptions(within(form).getByLabelText("Account"), "a-h");
        await user.selectOptions(await within(form).findByLabelText("Holding"), "h-1");
        await user.type(within(form).getByLabelText("Quantity"), "2");
        await user.type(within(form).getByLabelText("Unit price"), "100");
        await user.type(within(form).getByLabelText("Gross amount"), "200");
        await user.selectOptions(
          within(form).getByLabelText("Settlement currency"),
          "USD",
        );
      },
      expected: {
        kind: "buy",
        holdingId: "h-1",
        quantity: "2",
        unitPrice: "100",
        grossAmount: "200",
        settlementCurrency: "USD",
      },
    },
    {
      name: "balance adjustment",
      kindLabel: "Balance adjustment",
      fill: async (user: ReturnType<typeof userEvent.setup>, form: HTMLElement) => {
        await user.selectOptions(within(form).getByLabelText("Account"), "a-1");
        await user.type(within(form).getByLabelText("Amount"), "110000");
      },
      expected: {
        kind: "balance_adjustment",
        accountId: "a-1",
        amount: "110000",
        currency: "CNY",
      },
    },
  ])(
    "submits $name as tagged input without raw legs",
    async ({ kindLabel, fill, expected }) => {
      const user = userEvent.setup();
      mockActivityCatalog();
      mockPreviewAndCreate();
      await renderReadyApp();
      const form = await openActivityForm(user);
      await user.clear(within(form).getByLabelText("Date"));
      await user.type(within(form).getByLabelText("Date"), DATE);
      await user.clear(within(form).getByLabelText("Time"));
      await user.type(within(form).getByLabelText("Time"), TIME);
      if (kindLabel !== "Deposit") {
        await user.selectOptions(within(form).getByLabelText("Kind"), expected.kind);
      }
      await fill(user, form);
      await user.click(within(form).getByRole("button", { name: "Preview" }));
      expect(
        await screen.findByText("This preview is produced by Nestworth", {
          exact: false,
        }),
      ).toBeInTheDocument();
      await user.click(within(form).getByRole("button", { name: "Post activity" }));
      await waitFor(() => {
        expect(commands.createActivity).toHaveBeenCalledTimes(1);
      });
      const input = vi.mocked(commands.createActivity).mock.calls[0]?.[0];
      expect(input).toMatchObject(expected);
      expect(input).not.toHaveProperty("legs");
      expect(input).not.toHaveProperty("fxRate");
      expect(JSON.stringify(input)).not.toMatch(/externalFlow|resulting|grossTotal/);
      if (expected.kind === "transfer") {
        expect(within(form).queryByLabelText("FX rate")).not.toBeInTheDocument();
        expect(input).toMatchObject({ feeAmount: null, feeKind: null });
      }
    },
  );

  it("disables double submit while posting", async () => {
    const user = userEvent.setup();
    mockActivityCatalog();
    const pending = deferred<{
      status: "ok";
      data: ReturnType<typeof activityDetail>;
    }>();
    vi.mocked(commands.previewActivity).mockImplementation(async (input) => ({
      status: "ok",
      data: previewDto(input),
    }));
    vi.mocked(commands.createActivity).mockReturnValue(pending.promise);
    await renderReadyApp();
    const form = await openActivityForm(user);
    await fillDeposit(user, form);
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    await screen.findByText(/read-only/);
    await user.click(within(form).getByRole("button", { name: "Post activity" }));
    const posting = await screen.findByRole("button", { name: "Posting" });
    expect(posting).toBeDisabled();
    await user.click(posting);
    expect(commands.createActivity).toHaveBeenCalledTimes(1);
    pending.resolve({ status: "ok", data: activityDetail("act-1") });
  });

  it("focuses the first invalid field and preserves input after failures", async () => {
    const user = userEvent.setup();
    mockActivityCatalog();
    vi.mocked(commands.previewActivity).mockResolvedValue({
      status: "error",
      error: commandError("VALIDATION_ERROR", "Please check the highlighted fields.", {
        amount: "amount",
      }),
    });
    await renderReadyApp();
    const form = await openActivityForm(user);
    await user.clear(within(form).getByLabelText("Date"));
    await user.type(within(form).getByLabelText("Date"), DATE);
    await user.clear(within(form).getByLabelText("Time"));
    await user.type(within(form).getByLabelText("Time"), TIME);
    await user.selectOptions(within(form).getByLabelText("Account"), "a-1");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    expect(within(form).getByLabelText("Amount")).toHaveFocus();
    expect(commands.createActivity).not.toHaveBeenCalled();
    await user.type(within(form).getByLabelText("Amount"), "3000");
    vi.mocked(commands.previewActivity).mockImplementation(async (input) => ({
      status: "ok",
      data: previewDto(input),
    }));
    vi.mocked(commands.createActivity).mockResolvedValue({
      status: "error",
      error: commandError(
        "INSUFFICIENT_BALANCE",
        "This activity would make a balance negative.",
      ),
    });
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    await screen.findByText(/read-only/);
    await user.click(within(form).getByRole("button", { name: "Post activity" }));
    expect(
      await screen.findByText("This activity would make a balance negative."),
    ).toBeInTheDocument();
    expect(within(form).getByLabelText("Amount")).toHaveValue("3000");
  });

  it("invalidates a stale transfer preview when endpoints or currencies change", async () => {
    const user = userEvent.setup();
    mockActivityCatalog();
    mockPreviewAndCreate();
    await renderReadyApp();
    const form = await openActivityForm(user);
    await fillSharedTime(user, form);
    await user.selectOptions(within(form).getByLabelText("Kind"), "transfer");
    await user.selectOptions(within(form).getByLabelText("Source account"), "a-1");
    await user.selectOptions(within(form).getByLabelText("Destination account"), "a-2");
    await user.type(within(form).getByLabelText("Source amount"), "3000");
    await user.type(within(form).getByLabelText("Destination amount"), "3000");
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    await screen.findByText(/read-only/);
    await user.selectOptions(
      within(form).getByLabelText("Destination currency"),
      "USD",
    );
    expect(
      await screen.findByText("The activity changed after this preview", {
        exact: false,
      }),
    ).toBeInTheDocument();
    expect(within(form).getByRole("button", { name: "Post activity" })).toBeDisabled();
    await user.click(within(form).getByRole("button", { name: "Post activity" }));
    expect(commands.createActivity).not.toHaveBeenCalled();
  });

  it("requires reverse confirmation and restores focus on cancel", async () => {
    const user = userEvent.setup();
    mockActivityCatalog();
    const posted = activityDetail("act-1");
    vi.mocked(commands.listActivities).mockResolvedValue({
      status: "ok",
      data: { items: [posted], nextCursor: null, hasMore: false },
    });
    vi.mocked(commands.getActivity).mockResolvedValue({
      status: "ok",
      data: posted,
    });
    vi.mocked(commands.reverseActivity).mockResolvedValue({
      status: "ok",
      data: activityDetail("rev-1", {
        kind: "reversal",
        isReversal: true,
        reverses: "act-1",
      }),
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Activity" }));
    await user.click(await screen.findByRole("button", { name: "Open" }));
    const reverse = await screen.findByRole("button", { name: "Reverse" });
    await user.click(reverse);
    expect(commands.reverseActivity).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    expect(screen.getByRole("button", { name: "Reverse" })).toHaveFocus();
    await user.click(screen.getByRole("button", { name: "Reverse" }));
    await user.click(screen.getByRole("button", { name: "Post reversal" }));
    await waitFor(() => {
      expect(commands.reverseActivity).toHaveBeenCalledWith({
        id: "act-1",
        localDate: null,
        localTime: null,
        ambiguousOffset: null,
      });
    });
  });

  it("requires correction confirmation and restores focus on cancel", async () => {
    const user = userEvent.setup();
    mockActivityCatalog();
    mockPreviewAndCreate();
    const posted = activityDetail("act-1");
    vi.mocked(commands.listActivities).mockResolvedValue({
      status: "ok",
      data: { items: [posted], nextCursor: null, hasMore: false },
    });
    vi.mocked(commands.getActivity).mockResolvedValue({
      status: "ok",
      data: posted,
    });
    vi.mocked(commands.correctActivity).mockResolvedValue({
      status: "ok",
      data: {
        reversal: activityDetail("rev-1", { kind: "reversal", isReversal: true }),
        replacement: activityDetail("act-2", {
          isReplacement: true,
          corrects: "act-1",
        }),
      },
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Activity" }));
    await user.click(await screen.findByRole("button", { name: "Open" }));
    await user.click(await screen.findByRole("button", { name: "Correct" }));
    const form = (
      await screen.findByRole("heading", { name: "Replacement activity" })
    ).closest("form")!;
    await fillDeposit(user, form);
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    await screen.findByText(/read-only/);
    await user.click(within(form).getByRole("button", { name: "Correct" }));
    expect(commands.correctActivity).not.toHaveBeenCalled();
    await user.click(screen.getByRole("button", { name: "Cancel" }));
    await waitFor(() => {
      expect(within(form).getByRole("button", { name: "Correct" })).toHaveFocus();
    });
    await user.click(within(form).getByRole("button", { name: "Correct" }));
    await user.click(screen.getByRole("button", { name: "Post correction" }));
    await waitFor(() => {
      expect(commands.correctActivity).toHaveBeenCalledTimes(1);
    });
    const payload = vi.mocked(commands.correctActivity).mock.calls[0]?.[0];
    expect(payload?.originalId).toBe("act-1");
    expect(payload?.replacement.kind).toBe("deposit");
  });

  it("applies URL search filters", async () => {
    mockActivityCatalog();
    await renderReadyApp();
    router.history.replace("/activity?kind=deposit&accountId=a-1&start=2026-08-01");
    window.history.replaceState(
      null,
      "",
      "/activity?kind=deposit&accountId=a-1&start=2026-08-01",
    );
    expect(
      await screen.findByRole("heading", { name: "Activity" }),
    ).toBeInTheDocument();
    const filters = screen.getByRole("search", { name: "Activity filters" });
    expect(within(filters).getByLabelText("Kind")).toHaveValue("deposit");
    expect(within(filters).getByLabelText("Account")).toHaveValue("a-1");
    expect(within(filters).getByLabelText("Start date")).toHaveValue("2026-08-01");
    await waitFor(() => {
      expect(commands.listActivities).toHaveBeenCalledWith(
        expect.objectContaining({
          kind: "deposit",
          accountId: "a-1",
          startLocalDate: "2026-08-01",
          cursor: null,
        }),
      );
    });
  });

  it("appends on load more and replaces when filters change", async () => {
    const user = userEvent.setup();
    mockActivityCatalog();
    vi.mocked(commands.listActivities).mockImplementation(async (input) => {
      if (input.kind === "buy") {
        return {
          status: "ok",
          data: {
            items: [
              activityDetail("buy-1", {
                kind: "buy",
                classification: "trade_principal",
              }),
            ],
            nextCursor: null,
            hasMore: false,
          },
        };
      }
      if (input.cursor === "c1") {
        return {
          status: "ok",
          data: {
            items: [activityDetail("act-2", { kind: "withdrawal" })],
            nextCursor: null,
            hasMore: false,
          },
        };
      }
      return {
        status: "ok",
        data: {
          items: [activityDetail("act-1")],
          nextCursor: "c1",
          hasMore: true,
        },
      };
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Activity" }));
    expect(await screen.findByRole("heading", { name: "Deposit" })).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Load more" }));
    expect(
      await screen.findByRole("heading", { name: "Withdrawal" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Deposit" })).toBeInTheDocument();
    const filters = screen.getByRole("search", { name: "Activity filters" });
    await user.selectOptions(within(filters).getByLabelText("Kind"), "buy");
    expect(await screen.findByRole("heading", { name: "Buy" })).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Deposit" })).not.toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "Withdrawal" }),
    ).not.toBeInTheDocument();
  });

  it("shows loading, empty, error, pending, and badge states", async () => {
    const user = userEvent.setup();
    mockActivityCatalog();
    const pending = deferred<{
      status: "ok";
      data: {
        items: ReturnType<typeof activityDetail>[];
        nextCursor: string | null;
        hasMore: boolean;
      };
    }>();
    vi.mocked(commands.listActivities).mockReturnValue(pending.promise);
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Activity" }));
    expect(await screen.findByRole("status")).toHaveTextContent("Loading…");
    pending.resolve({
      status: "ok",
      data: { items: [], nextCursor: null, hasMore: false },
    });
    expect(await screen.findByText("No activities yet.")).toBeInTheDocument();
    vi.mocked(commands.listActivities).mockResolvedValue({
      status: "error",
      error: commandError("DATABASE_UNAVAILABLE", "The database is unavailable."),
    });
    await user.selectOptions(
      within(screen.getByRole("search", { name: "Activity filters" })).getByLabelText(
        "Kind",
      ),
      "fee",
    );
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The database is unavailable.",
    );
  });

  it("renders reversed, reversal, corrected, and archived-reference badges", async () => {
    const user = userEvent.setup();
    mockActivityCatalog();
    vi.mocked(commands.listAccounts).mockResolvedValue({
      status: "ok",
      data: [
        accountRecord("a-1", "DBS Savings", { archivedAt: "2026-08-18T00:00:00.000Z" }),
      ],
    });
    vi.mocked(commands.listActivities).mockResolvedValue({
      status: "ok",
      data: {
        items: [
          activityDetail("act-1", {
            reversed: true,
            legs: [
              {
                id: "leg-1",
                accountId: "a-1",
                accountName: "DBS Savings",
                role: "destination",
                direction: "increase",
                componentKind: "account_value",
                classification: "external_inflow",
                amount: "3000",
                currency: "CNY",
                holdingId: null,
                instrumentId: null,
                instrumentName: null,
                quantity: null,
                fxRate: null,
                sortOrder: 0,
              },
            ],
          }),
          activityDetail("rev-1", {
            kind: "reversal",
            isReversal: true,
            classification: "external_outflow",
          }),
          activityDetail("act-2", {
            isReplacement: true,
            corrects: "act-1",
            classification: "external_inflow",
          }),
        ],
        nextCursor: null,
        hasMore: false,
      },
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Activity" }));
    expect(await screen.findByText("Reversed")).toBeInTheDocument();
    expect(screen.getAllByText("Reversal").length).toBeGreaterThan(0);
    expect(screen.getAllByText("Corrected").length).toBeGreaterThan(0);
    expect(screen.getByText("Archived reference")).toBeInTheDocument();
  });

  it("blocks posting until the history timezone is confirmed", async () => {
    const user = userEvent.setup();
    mockActivityCatalog();
    mockPreviewAndCreate();
    vi.mocked(commands.getHistoryOrigin).mockResolvedValue({
      status: "ok",
      data: { ...confirmedHistoryOrigin(), timezoneConfirmed: false },
    });
    vi.mocked(commands.confirmHistoryTimezone).mockResolvedValue({
      status: "ok",
      data: confirmedHistoryOrigin(),
    });
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Activity" }));
    expect(await screen.findByText("Confirm history timezone")).toBeInTheDocument();
    expect(await screen.findByText("No activities yet.")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "New Activity" }));
    expect(
      screen.queryByRole("button", { name: "Post activity" }),
    ).not.toBeInTheDocument();
    vi.mocked(commands.getHistoryOrigin).mockResolvedValue({
      status: "ok",
      data: confirmedHistoryOrigin(),
    });
    await user.click(screen.getByRole("button", { name: "Confirm UTC" }));
    await waitFor(() => {
      expect(commands.confirmHistoryTimezone).toHaveBeenCalledWith({ timezone: "UTC" });
    });
    expect(await screen.findByRole("button", { name: "Preview" })).toBeInTheDocument();
  });

  it("can complete deposit creation from the keyboard", async () => {
    const user = userEvent.setup();
    mockActivityCatalog();
    mockPreviewAndCreate();
    await renderReadyApp();
    const form = await openActivityForm(user);
    await fillDeposit(user, form);
    await user.click(within(form).getByRole("button", { name: "Preview" }));
    await screen.findByText(/read-only/);
    await user.keyboard("{Tab}");
    await user.keyboard("{Enter}");
    await waitFor(() => {
      expect(commands.createActivity).toHaveBeenCalledTimes(1);
    });
  });
});

async function openActivityForm(user: ReturnType<typeof userEvent.setup>) {
  await user.click(await screen.findByRole("link", { name: "Activity" }));
  await user.click(await screen.findByRole("button", { name: "New Activity" }));
  const heading = await screen.findByRole("heading", { name: "New Activity" });
  return heading.closest("form")!;
}

async function fillSharedTime(
  user: ReturnType<typeof userEvent.setup>,
  form: HTMLElement,
) {
  await user.clear(within(form).getByLabelText("Date"));
  await user.type(within(form).getByLabelText("Date"), DATE);
  await user.clear(within(form).getByLabelText("Time"));
  await user.type(within(form).getByLabelText("Time"), TIME);
}

async function fillDeposit(
  user: ReturnType<typeof userEvent.setup>,
  form: HTMLElement,
) {
  await fillSharedTime(user, form);
  await user.type(within(form).getByLabelText("Amount"), "3000");
  await user.selectOptions(within(form).getByLabelText("Account"), "a-1");
}

function mockActivityCatalog() {
  vi.mocked(commands.listAccounts).mockResolvedValue({
    status: "ok",
    data: [
      accountRecord("a-1", "DBS Savings"),
      accountRecord("a-2", "WeChat"),
      accountRecord("a-h", "Brokerage", {
        trackingMode: "holdings",
        primaryCategory: "investment",
        secondaryCategory: "brokerage_account",
        defaultCurrency: "USD",
      }),
    ],
  });
  vi.mocked(commands.listHoldings).mockResolvedValue({
    status: "ok",
    data: [holdingRecord()],
  });
}

function mockPreviewAndCreate() {
  vi.mocked(commands.previewActivity).mockImplementation(async (input) => ({
    status: "ok",
    data: previewDto(input),
  }));
  vi.mocked(commands.createActivity).mockImplementation(async (input) => ({
    status: "ok",
    data: activityDetail("act-1", { kind: input.kind }),
  }));
}

function previewDto(input: CreateActivityInput): ActivityPreviewDto {
  return {
    activity: activityDetail("preview-1", {
      kind: input.kind,
      classification:
        input.kind === "deposit"
          ? "external_inflow"
          : input.kind === "transfer"
            ? "internal_transfer"
            : input.kind === "buy"
              ? "trade_principal"
              : "remeasurement",
      effectiveLocalDate: input.localDate,
    }),
    resulting: [
      {
        accountId: "a-1",
        accountName: "DBS Savings",
        componentKind: "account_value",
        holdingId: null,
        currency: "CNY",
        before: "10000",
        after: "13000",
      },
    ],
  };
}

function holdingRecord(): HoldingRecordDto {
  return {
    id: "h-1",
    accountId: "a-h",
    instrumentId: "ins-1",
    instrumentName: "QQQ",
    instrumentSymbol: "QQQ",
    quoteCurrency: "USD",
    quantity: "0",
    note: null,
    sortOrder: 0,
    createdAt: "2026-08-17T00:00:00.000Z",
    updatedAt: "2026-08-17T00:00:00.000Z",
    archivedAt: null,
  };
}
