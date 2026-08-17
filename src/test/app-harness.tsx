import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";

import App from "@/App";
import { router } from "@/app/router";
import type {
  AccountRecordDto,
  BootstrapDto,
  CommandError,
  ErrorCode,
  GroupRecordDto,
  InstitutionRecordDto,
  MemberRecordDto,
} from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";

export const emptyBootstrap: BootstrapDto = {
  status: "ready",
  onboardingRequired: true,
  settings: { language: "system", appearance: "system", lastHouseholdId: null },
  household: null,
  members: [],
};

export const readyBootstrap: BootstrapDto = {
  status: "ready",
  onboardingRequired: false,
  settings: { language: "system", appearance: "system", lastHouseholdId: "hh-1" },
  household: { id: "hh-1", name: "Wang Family", baseCurrency: "CNY" },
  members: [
    { id: "m-1", name: "Walt" },
    { id: "m-2", name: "Spouse" },
  ],
};

export const blockedBootstrap: BootstrapDto = {
  status: "blocked",
  error: {
    code: "UNSUPPORTED_NEWER_DATABASE",
    message: "This database was created by a newer version of Nestworth.",
    fields: { foundMigration: "999", supportedMigration: "1" },
  },
  databasePath: "/tmp/nestworth.sqlite3",
  foundMigration: 999,
  supportedMigration: 1,
};

const TIMESTAMP = "2026-08-17T00:00:00.000Z";

export function memberRecord(
  id: string,
  name: string,
  sortOrder: number,
  archivedAt: string | null = null,
): MemberRecordDto {
  return {
    id,
    name,
    note: null,
    avatarAssetId: null,
    sortOrder,
    createdAt: TIMESTAMP,
    updatedAt: TIMESTAMP,
    archivedAt,
  };
}

export function institutionRecord(
  id: string,
  name: string,
  extras: Partial<InstitutionRecordDto> = {},
): InstitutionRecordDto {
  return {
    id,
    name,
    institutionType: null,
    countryCode: null,
    website: null,
    note: null,
    logoAssetId: null,
    sortOrder: 0,
    createdAt: TIMESTAMP,
    updatedAt: TIMESTAMP,
    archivedAt: null,
    ...extras,
  };
}

export function groupRecord(
  id: string,
  name: string,
  extras: Partial<GroupRecordDto> = {},
): GroupRecordDto {
  return {
    id,
    name,
    iconKey: null,
    color: null,
    description: null,
    logoAssetId: null,
    sortOrder: 0,
    createdAt: TIMESTAMP,
    updatedAt: TIMESTAMP,
    archivedAt: null,
    ...extras,
  };
}

export function accountRecord(
  id: string,
  name: string,
  extras: Partial<AccountRecordDto> = {},
): AccountRecordDto {
  return {
    id,
    name,
    primaryCategory: "cash_equivalent",
    secondaryCategory: "bank_account",
    trackingMode: "balance",
    defaultCurrency: "CNY",
    institutionId: "i-1",
    groupId: null,
    note: null,
    logoAssetId: null,
    includeInNetWorth: true,
    includeInInvestment: false,
    includeInLiquidAssets: true,
    openedOn: null,
    closedOn: null,
    sortOrder: 0,
    createdAt: TIMESTAMP,
    updatedAt: TIMESTAMP,
    archivedAt: null,
    latestValue: { amount: "100000", currency: "CNY" },
    owners: [{ memberId: "m-1", memberName: "Walt", shareBps: 10_000 }],
    ...extras,
  };
}

export function commandError(
  code: ErrorCode,
  message: string,
  fields: CommandError["fields"] = null,
): CommandError {
  return { code, message, fields };
}

export function mockBootstrap(data: BootstrapDto) {
  vi.mocked(commands.bootstrap).mockResolvedValue({ status: "ok", data });
}

export function resetCommandMocks() {
  for (const command of Object.values(commands)) {
    vi.mocked(command).mockReset();
  }
  vi.mocked(commands.listMembers).mockResolvedValue({ status: "ok", data: [] });
  vi.mocked(commands.listInstitutions).mockResolvedValue({ status: "ok", data: [] });
  vi.mocked(commands.listGroups).mockResolvedValue({ status: "ok", data: [] });
  vi.mocked(commands.listAccounts).mockResolvedValue({ status: "ok", data: [] });
  vi.mocked(commands.getAccount).mockResolvedValue({
    status: "error",
    error: commandError("NOT_FOUND", "missing"),
  });
}

export async function resetApp() {
  resetCommandMocks();
  router.history.replace("/");
  window.history.replaceState(null, "", "/");
}

export async function renderApp() {
  render(<App />);
}

export async function renderReadyApp() {
  mockBootstrap(readyBootstrap);
  router.history.replace("/overview");
  window.history.replaceState(null, "", "/overview");
  await renderApp();
  await screen.findByRole("navigation", { name: "Household" });
}

export async function completeValidOnboarding(
  user: ReturnType<typeof userEvent.setup>,
) {
  await screen.findByRole("heading", { name: "Set up your household" });
  await user.type(screen.getByLabelText("Household name"), "Wang Family");
  await user.click(screen.getByRole("button", { name: "Next" }));
  await user.click(screen.getByRole("button", { name: "Next" }));
  await user.type(screen.getByLabelText("Member 1"), "Walt");
  await user.click(screen.getByRole("button", { name: "Next" }));
  await user.click(screen.getByRole("button", { name: "Finish" }));
}

export function deferred<T>() {
  let resolve!: (value: T) => void;
  const promise = new Promise<T>((res) => {
    resolve = res;
  });
  return { promise, resolve };
}
