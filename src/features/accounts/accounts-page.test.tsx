import { screen, waitFor } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { beforeEach, describe, expect, it, vi } from "vitest";

import type {
  AccountRecordDto,
  InstitutionRecordDto,
} from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import {
  accountRecord,
  commandError,
  deferred,
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
    expect(screen.getByText("Walt")).toBeInTheDocument();
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
});

function mockAccountStore(accounts: AccountRecordDto[]) {
  const institutions: InstitutionRecordDto[] = [institutionRecord("i-1", "DBS")];
  vi.mocked(commands.listInstitutions).mockResolvedValue({
    status: "ok",
    data: institutions,
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
      latestValue: { amount: input.initialAmount, currency: input.defaultCurrency },
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
