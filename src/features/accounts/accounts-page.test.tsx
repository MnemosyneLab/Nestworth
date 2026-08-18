import { screen, waitFor, within } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import { router } from "@/app/router";
import type {
  AccountRecordDto,
  GroupRecordDto,
  InstitutionRecordDto,
} from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import {
  accountRecord,
  commandError,
  deferred,
  emptyValuation,
  groupRecord,
  institutionRecord,
  renderReadyApp,
  resetApp,
} from "@/test/app-harness";

describe("accounts page", () => {
  beforeEach(async () => {
    await resetApp();
  });

  it("creates a bank account with owner, institution, and initial value", async () => {
    const user = userEvent.setup();
    const accounts: AccountRecordDto[] = [];
    mockAccountStore(accounts);
    await renderReadyApp();
    await user.click(await screen.findByRole("link", { name: "Add account" }));
    expect(
      await screen.findByText("Add an account to start tracking net worth."),
    ).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add account" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await user.type(screen.getByLabelText("Name"), "DBS Savings");
    await user.selectOptions(screen.getByLabelText("Institution"), "i-1");
    await user.type(screen.getByLabelText("Amount"), "100000");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(
      await screen.findByRole("heading", { name: "DBS Savings" }),
    ).toBeInTheDocument();
    expect(screen.getByText("CNY 100,000")).toBeInTheDocument();
    expect(screen.getByText("Bank Account")).toBeInTheDocument();
    expect(
      within(
        screen.getByRole("heading", { name: "DBS Savings" }).closest("article")!,
      ).getByText("Walt"),
    ).toBeInTheDocument();
    expect(commands.createAccount).toHaveBeenCalledWith({
      name: "DBS Savings",
      primaryCategory: "cash_equivalent",
      secondaryCategory: "bank_account",
      defaultCurrency: "CNY",
      institutionId: "i-1",
      groupId: null,
      trackingMode: "balance",
      note: null,
      includeInNetWorth: true,
      includeInInvestment: false,
      includeInLiquidAssets: true,
      openedOn: null,
      closedOn: null,
      owners: [{ memberId: "m-1", percent: null, shareBps: 10_000 }],
      initialAmount: "100000",
    });
  });

  it("opens account detail and updates the latest value", async () => {
    const user = userEvent.setup();
    const accounts = [accountRecord("a-1", "DBS Savings")];
    mockAccountStore(accounts);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Accounts" }));
    await screen.findByRole("heading", { name: "DBS Savings" });
    await user.click(screen.getByRole("link", { name: "Open" }));
    expect(
      await screen.findByRole("heading", { name: "Update value" }),
    ).toBeInTheDocument();
    await user.type(screen.getByLabelText("Amount"), "110000");
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(await screen.findByText("CNY 110,000")).toBeInTheDocument();
    expect(commands.updateAccountValue).toHaveBeenCalledWith({
      id: "a-1",
      amount: "110000",
    });
  });

  it("archives, shows archived, and restores an account", async () => {
    const user = userEvent.setup();
    const accounts = [accountRecord("a-1", "DBS Savings")];
    mockAccountStore(accounts);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Accounts" }));
    await user.click(
      await screen.findByRole("button", { name: "Archive DBS Savings" }),
    );
    await waitFor(() => {
      expect(
        screen.queryByRole("heading", { name: "DBS Savings" }),
      ).not.toBeInTheDocument();
    });
    await user.click(screen.getByLabelText("Show archived"));
    expect(
      await screen.findByRole("heading", { name: "DBS Savings" }),
    ).toBeInTheDocument();
    expect(screen.getByText("Archived")).toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Restore DBS Savings" }));
    await user.click(screen.getByLabelText("Show archived"));
    expect(
      await screen.findByRole("heading", { name: "DBS Savings" }),
    ).toBeInTheDocument();
    expect(screen.queryByText("Archived")).not.toBeInTheDocument();
  });

  it("focuses name when client validation fails", async () => {
    const user = userEvent.setup();
    mockAccountStore([]);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Accounts" }));
    await user.click(await screen.findByRole("button", { name: "Add account" }));
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    expect(screen.getByText("This field is required.")).toBeInTheDocument();
    expect(commands.createAccount).not.toHaveBeenCalled();
  });

  it("does not save twice while the mutation is pending", async () => {
    const user = userEvent.setup();
    mockAccountStore([]);
    const pending = deferred<{ status: "ok"; data: AccountRecordDto }>();
    vi.mocked(commands.createAccount).mockReturnValue(pending.promise);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Accounts" }));
    await user.click(await screen.findByRole("button", { name: "Add account" }));
    await user.type(screen.getByLabelText("Name"), "DBS Savings");
    await user.type(screen.getByLabelText("Amount"), "100000");
    await user.click(screen.getByRole("button", { name: "Save" }));
    const saving = await screen.findByRole("button", { name: "Saving" });
    expect(saving).toBeDisabled();
    await user.click(saving);
    expect(commands.createAccount).toHaveBeenCalledTimes(1);
    pending.resolve({ status: "ok", data: accountRecord("a-1", "DBS Savings") });
  });

  it("nests All, member, and Shared links under Accounts", async () => {
    await renderReadyApp();
    const nav = screen.getByRole("navigation", { name: "Household" });
    expect(within(nav).getByRole("link", { name: "Accounts" })).toBeInTheDocument();
    expect(within(nav).getByRole("link", { name: "All" })).toBeInTheDocument();
    expect(within(nav).getByRole("link", { name: "Walt" })).toBeInTheDocument();
    expect(within(nav).getByRole("link", { name: "Spouse" })).toBeInTheDocument();
    expect(within(nav).getByRole("link", { name: "Shared" })).toBeInTheDocument();
  });

  it("filters sole-owned and shared accounts from the sidebar", async () => {
    const user = userEvent.setup();
    mockAccountStore(householdAccounts());
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Walt" }));
    expect(
      await screen.findByRole("heading", { name: "DBS Savings" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "WeChat" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Home" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("link", { name: "Shared" }));
    expect(await screen.findByRole("heading", { name: "Home" })).toBeInTheDocument();
    expect(
      screen.queryByRole("heading", { name: "DBS Savings" }),
    ).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "WeChat" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("link", { name: "All" }));
    expect(
      await screen.findByRole("heading", { name: "DBS Savings" }),
    ).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "WeChat" })).toBeInTheDocument();
    expect(screen.getByRole("heading", { name: "Home" })).toBeInTheDocument();
  });

  it("restores the owner view from the URL on refresh", async () => {
    mockAccountStore(householdAccounts());
    await renderReadyApp();
    router.history.replace("/accounts?owner=m-1");
    window.history.replaceState(null, "", "/accounts?owner=m-1");
    expect(
      await screen.findByRole("heading", { name: "DBS Savings" }),
    ).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Home" })).not.toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "WeChat" })).not.toBeInTheDocument();
    expect(screen.getByRole("link", { name: "Walt" })).toHaveAttribute(
      "aria-current",
      "page",
    );
    expect(window.location.search).toContain("owner=m-1");
  });

  it("can add an account with the keyboard", async () => {
    const user = userEvent.setup();
    const accounts: AccountRecordDto[] = [];
    mockAccountStore(accounts);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Accounts" }));
    await user.click(await screen.findByRole("button", { name: "Add account" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
    await user.keyboard("DBS Savings");
    await user.type(screen.getByLabelText("Amount"), "100000");
    await user.keyboard("{Enter}");
    expect(
      await screen.findByRole("heading", { name: "DBS Savings" }),
    ).toBeInTheDocument();
    expect(commands.createAccount).toHaveBeenCalledTimes(1);
  });

  it("shows list errors with role=alert", async () => {
    const user = userEvent.setup();
    vi.mocked(commands.listAccounts).mockResolvedValue({
      status: "error",
      error: commandError("DATABASE_UNAVAILABLE", "The database is unavailable."),
    });
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Accounts" }));
    expect(await screen.findByRole("alert")).toHaveTextContent(
      "The database is unavailable.",
    );
  });

  it("filters by category and keeps the owner param", async () => {
    const user = userEvent.setup();
    mockAccountStore(householdAccounts());
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Shared" }));
    await screen.findByRole("heading", { name: "Home" });
    const filters = screen.getByRole("search", { name: "Account filters" });
    await user.selectOptions(within(filters).getByLabelText("Category"), "property");
    expect(screen.getByRole("heading", { name: "Home" })).toBeInTheDocument();
    await user.selectOptions(
      within(filters).getByLabelText("Category"),
      "cash_equivalent",
    );
    expect(
      await screen.findByText("No accounts match these filters."),
    ).toBeInTheDocument();
    expect(screen.queryByRole("heading", { name: "Home" })).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Add account" }));
    expect(screen.getByLabelText("Name")).toHaveFocus();
  });

  it("creates a holdings investment account without an initial amount", async () => {
    const user = userEvent.setup();
    const accounts: AccountRecordDto[] = [];
    mockAccountStore(accounts);
    await renderReadyApp();
    await user.click(screen.getByRole("link", { name: "Accounts" }));
    await user.click(await screen.findByRole("button", { name: "Add account" }));
    await user.type(screen.getByLabelText("Name"), "Brokerage");
    await user.selectOptions(screen.getByLabelText("Category"), "brokerage_account");
    expect(screen.queryByLabelText("Amount")).not.toBeInTheDocument();
    await user.click(screen.getByRole("button", { name: "Save" }));
    expect(
      await screen.findByRole("heading", { name: "Brokerage" }),
    ).toBeInTheDocument();
    expect(commands.createAccount).toHaveBeenCalledWith(
      expect.objectContaining({
        name: "Brokerage",
        primaryCategory: "investment",
        secondaryCategory: "brokerage_account",
        trackingMode: "holdings",
        initialAmount: null,
        defaultCurrency: "CNY",
      }),
    );
  });
});

function householdAccounts(): AccountRecordDto[] {
  return [
    accountRecord("a-1", "DBS Savings"),
    accountRecord("a-2", "WeChat", {
      institutionId: null,
      secondaryCategory: "digital_wallet",
      latestValue: { amount: "10000", currency: "CNY" },
      owners: [{ memberId: "m-2", memberName: "Spouse", shareBps: 10_000 }],
    }),
    accountRecord("a-3", "Home", {
      primaryCategory: "property",
      secondaryCategory: "real_estate",
      institutionId: null,
      groupId: "g-1",
      latestValue: { amount: "4000000", currency: "CNY" },
      owners: [
        { memberId: "m-1", memberName: "Walt", shareBps: 5_000 },
        { memberId: "m-2", memberName: "Spouse", shareBps: 5_000 },
      ],
    }),
  ];
}

function mockAccountStore(accounts: AccountRecordDto[]) {
  const institutions: InstitutionRecordDto[] = [institutionRecord("i-1", "DBS")];
  const groups: GroupRecordDto[] = [groupRecord("g-1", "Emergency")];
  vi.mocked(commands.listInstitutions).mockResolvedValue({
    status: "ok",
    data: institutions,
  });
  vi.mocked(commands.listGroups).mockResolvedValue({
    status: "ok",
    data: groups,
  });
  vi.mocked(commands.listAccounts).mockImplementation(async (input) => ({
    status: "ok",
    data: input.includeArchived
      ? [...accounts]
      : accounts.filter((account) => account.archivedAt === null),
  }));
  vi.mocked(commands.getAccount).mockImplementation(async (input) => {
    const account = accounts.find((item) => item.id === input.id);
    if (!account) {
      return { status: "error", error: commandError("NOT_FOUND", "missing") };
    }
    return { status: "ok", data: { ...account, latestValue: account.latestValue } };
  });
  vi.mocked(commands.createAccount).mockImplementation(async (input) => {
    const created = accountRecord(`a-${accounts.length + 1}`, input.name, {
      institutionId: input.institutionId,
      latestValue: input.initialAmount
        ? { amount: input.initialAmount, currency: input.defaultCurrency }
        : null,
      owners: input.owners.map((owner) => ({
        memberId: owner.memberId,
        memberName: owner.memberId === "m-1" ? "Walt" : "Spouse",
        shareBps: owner.shareBps ?? 10_000,
      })),
    });
    accounts.push(created);
    return { status: "ok", data: created };
  });
  vi.mocked(commands.updateAccountValue).mockImplementation(async (input) => {
    const account = accounts.find((item) => item.id === input.id);
    if (!account) {
      return { status: "error", error: commandError("NOT_FOUND", "missing") };
    }
    const updated = {
      ...account,
      latestValue: { amount: input.amount, currency: account.defaultCurrency },
      valuation: emptyValuation(account.defaultCurrency, input.amount),
    };
    Object.assign(account, updated);
    return { status: "ok", data: { ...updated } };
  });
  vi.mocked(commands.archiveAccount).mockImplementation(async (input) => {
    const account = accounts.find((item) => item.id === input.id);
    if (!account) {
      return { status: "error", error: commandError("NOT_FOUND", "missing") };
    }
    account.archivedAt = "2026-08-17T01:00:00.000Z";
    return { status: "ok", data: account };
  });
  vi.mocked(commands.restoreAccount).mockImplementation(async (input) => {
    const account = accounts.find((item) => item.id === input.id);
    if (!account) {
      return { status: "error", error: commandError("NOT_FOUND", "missing") };
    }
    account.archivedAt = null;
    return { status: "ok", data: account };
  });
}
