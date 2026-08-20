import { render, screen } from "@testing-library/react";
import userEvent from "@testing-library/user-event";
import { vi } from "vitest";

import App from "@/App";
import { router } from "@/app/router";
import type {
  AccountRecordDto,
  AccountTimelinePageDto,
  AccountValuationDto,
  ActivityDetailDto,
  ActivityPageDto,
  AnalyticsStatusDto,
  BootstrapDto,
  CommandError,
  CostBasisDeclarationPageDto,
  ErrorCode,
  GainSummaryIpcDto,
  GroupRecordDto,
  HistoryOriginDto,
  HistoryStatusDto,
  HoldingLotPageDto,
  InstitutionRecordDto,
  MemberRecordDto,
  MoneyAvailabilityDto,
  NetWorthAttributionIpcDto,
  NetWorthTrendDto,
  PerformanceSummaryDto,
  RebuildHistorySnapshotsResultDto,
  OverviewDto,
  PortfolioDto,
  ReferenceCatalogDto,
  SignedMoneyAvailabilityDto,
} from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";

export const testReferenceCatalog: ReferenceCatalogDto = {
  currencies: [
    ...["CNY", "USD", "HKD", "SGD", "EUR", "JPY", "TWD", "KRW", "GBP"].map((value) => ({
      value,
      group: "core",
    })),
    ...["AUD", "NZD", "INR", "IDR", "MYR", "THB", "VND", "PHP", "BND"].map((value) => ({
      value,
      group: "asiaPacific",
    })),
  ],
  countries: [
    ...["CN", "HK", "MO", "TW", "JP", "KR", "SG", "US", "GB"].map((value) => ({
      value,
      group: value === "US" ? "americas" : value === "GB" ? "europe" : "asiaMiddleEast",
    })),
    { value: "AU", group: "oceania" },
  ],
  institutionTypes: [
    { value: "bank", group: "financial" },
    { value: "digital_bank", group: "financial" },
    { value: "brokerage", group: "financial" },
    { value: "internet_platform", group: "platform" },
    { value: "other", group: "other" },
  ],
  groupIcons: ["wallet", "home", "shield", "briefcase", "heart", "star"],
  groupColors: ["#2563EB", "#16A34A", "#DC2626", "#D97706", "#7C3AED", "#0F766E"],
  languages: ["system", "en", "zh-CN"],
  appearances: ["system", "light", "dark"],
};

export const emptyBootstrap: BootstrapDto = {
  status: "ready",
  onboardingRequired: true,
  settings: { language: "system", appearance: "system", lastHouseholdId: null },
  household: null,
  members: [],
  referenceCatalog: testReferenceCatalog,
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
  referenceCatalog: testReferenceCatalog,
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

export function emptyValuation(currency = "CNY", amount = "0"): AccountValuationDto {
  const money = { amount, currency };
  return {
    native: money,
    base: money,
    complete: true,
    freshness: "manual",
    unvaluedItems: [],
  };
}

export function accountRecord(
  id: string,
  name: string,
  extras: Partial<AccountRecordDto> = {},
): AccountRecordDto {
  const latestValue =
    extras.latestValue === undefined
      ? { amount: "100000", currency: "CNY" }
      : extras.latestValue;
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
    owners: [{ memberId: "m-1", memberName: "Walt", shareBps: 10_000 }],
    ...extras,
    latestValue,
    valuation: extras.valuation ?? emptyValuation("CNY", latestValue?.amount ?? "0"),
  };
}

export function emptyOverview(currency = "CNY"): OverviewDto {
  const zero = { amount: "0", currency };
  return {
    baseCurrency: currency,
    accountCount: 0,
    assets: zero,
    liabilities: zero,
    netWorth: zero,
    byCategory: [],
    byMember: [],
    byInstitution: [],
    byGroup: [],
    isComplete: true,
    unvaluedItems: [],
  };
}

export function emptyPortfolio(currency = "CNY"): PortfolioDto {
  return {
    baseCurrency: currency,
    total: { amount: "0", currency },
    isComplete: true,
    coverageBps: 10_000,
    unvaluedItems: [],
    positions: [],
    accounts: [],
    cash: [],
    byCurrency: [],
    byCountry: [],
    byInstrumentType: [],
    requiredFx: [],
  };
}

export function confirmedHistoryOrigin(): HistoryOriginDto {
  return {
    id: "origin-1",
    timezone: "UTC",
    timezoneConfirmed: true,
    originAt: TIMESTAMP,
    originLocalDate: "2026-08-17",
    source: "onboarding",
    schemaVersion: 3,
    createdAt: TIMESTAMP,
    accountValues: [],
    cashValues: [],
    holdings: [],
  };
}

export function emptyActivityPage(): ActivityPageDto {
  return { items: [], nextCursor: null, hasMore: false };
}

export function emptyTimelinePage(): AccountTimelinePageDto {
  return { items: [], nextCursor: null, hasMore: false };
}

export function emptyHistoryStatus(): HistoryStatusDto {
  return {
    timezone: "UTC",
    timezoneConfirmed: true,
    originAt: TIMESTAMP,
    originLocalDate: "2026-08-17",
    dirtyFrom: null,
    lastCompletedOn: null,
    lastClosedOn: null,
    rebuildStatus: "idle",
    rebuildCursorOn: null,
  };
}

export function emptyRebuildResult(): RebuildHistorySnapshotsResultDto {
  return {
    processedDays: 0,
    remaining: false,
    cancelled: false,
    dirtyFrom: null,
    lastCompletedOn: null,
    status: "idle",
  };
}

export function emptyAnalyticsStatus(): AnalyticsStatusDto {
  return {
    usableHistory: {
      kind: "unavailable",
      reason: "ANALYTICS_PERIOD_UNAVAILABLE",
      blockingDates: [],
    },
    earliestCompleteSnapshotOn: {
      kind: "unavailable",
      reason: "ANALYTICS_PERIOD_UNAVAILABLE",
      blockingDates: [],
    },
    blockingDates: [],
    unknownBasisLotCount: 0,
    unknownBasisValue: {
      kind: "unavailable",
      reason: "ANALYTICS_INPUT_INCOMPLETE",
      blockingDates: [],
    },
  };
}

function unavailableSignedMoney(): SignedMoneyAvailabilityDto {
  return {
    kind: "unavailable",
    reason: "ANALYTICS_INPUT_INCOMPLETE",
    blockingDates: [],
  };
}

function unavailableMoney(): MoneyAvailabilityDto {
  return {
    kind: "unavailable",
    reason: "ANALYTICS_INPUT_INCOMPLETE",
    blockingDates: [],
  };
}

export function emptyPerformanceSummary(): PerformanceSummaryDto {
  return {
    twr: {
      kind: "unavailable",
      reason: "ANALYTICS_PERIOD_UNAVAILABLE",
      blockingDates: [],
    },
    xirr: {
      kind: "unavailable",
      reason: "ANALYTICS_PERIOD_UNAVAILABLE",
      blockingDates: [],
    },
  };
}

export function emptyGainSummary(): GainSummaryIpcDto {
  return {
    realizedGross: unavailableSignedMoney(),
    realizedNet: unavailableSignedMoney(),
    allocatedFees: unavailableSignedMoney(),
    unrealizedGross: unavailableSignedMoney(),
    unexplainedDisposal: unavailableSignedMoney(),
    basisComplete: true,
    inputComplete: false,
    decompositionComplete: false,
    unknownBasisQuantity: "0",
    unknownBasisValue: unavailableMoney(),
    instrumentMovement: unavailableSignedMoney(),
    currencyMovement: unavailableSignedMoney(),
    unrealizedAsOf: "currentSnapshot",
    income: [],
    fees: [],
  };
}

export function emptyAttribution(): NetWorthAttributionIpcDto {
  return {
    kind: "unavailable",
    reason: "ANALYTICS_PERIOD_UNAVAILABLE",
    blockingDates: [],
    unconvertibleFlowCount: 0,
  };
}

export function emptyLotPage(): HoldingLotPageDto {
  return { items: [], nextCursor: null, hasMore: false };
}

export function emptyDeclarationPage(): CostBasisDeclarationPageDto {
  return { items: [], nextCursor: null, hasMore: false };
}

export function emptyNetWorthTrend(currency = "CNY"): NetWorthTrendDto {
  const zero = { amount: "0", currency };
  return {
    baseCurrency: currency,
    range: "all",
    originLocalDate: "2026-08-17",
    dirtyFrom: null,
    points: [],
    current: {
      date: null,
      asOf: TIMESTAMP,
      assets: zero,
      liabilities: zero,
      netWorth: zero,
      isComplete: true,
      isLive: true,
      coverageBps: 10_000,
      missingCount: 0,
      valuedComponentCount: 0,
      totalComponentCount: 0,
    },
  };
}

export function activityDetail(
  id: string,
  extras: Partial<ActivityDetailDto> = {},
): ActivityDetailDto {
  return {
    id,
    kind: "deposit",
    classification: "external_inflow",
    effectiveAt: TIMESTAMP,
    effectiveLocalDate: "2026-08-17",
    createdAt: TIMESTAMP,
    note: null,
    reverses: null,
    corrects: null,
    correctionGroup: null,
    incomeKind: null,
    feeKind: null,
    relatedInstrumentId: null,
    reversed: false,
    isReversal: false,
    isReplacement: false,
    legs: [],
    chain: { originalId: id, reversalId: null, replacementId: null },
    fxConversion: null,
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
  vi.mocked(commands.getOverview).mockResolvedValue({
    status: "ok",
    data: emptyOverview(),
  });
  vi.mocked(commands.getSettings).mockResolvedValue({
    status: "ok",
    data: {
      language: "system",
      appearance: "system",
      lastHouseholdId: "hh-1",
    },
  });
  vi.mocked(commands.updateSettings).mockResolvedValue({
    status: "ok",
    data: {
      language: "system",
      appearance: "system",
      lastHouseholdId: "hh-1",
    },
  });
  vi.mocked(commands.getMedia).mockResolvedValue({
    status: "error",
    error: commandError("NOT_FOUND", "missing"),
  });
  vi.mocked(commands.setMemberAvatar).mockResolvedValue({
    status: "error",
    error: commandError("NOT_FOUND", "missing"),
  });
  vi.mocked(commands.setInstitutionLogo).mockResolvedValue({
    status: "error",
    error: commandError("NOT_FOUND", "missing"),
  });
  vi.mocked(commands.setGroupLogo).mockResolvedValue({
    status: "error",
    error: commandError("NOT_FOUND", "missing"),
  });
  vi.mocked(commands.setAccountLogo).mockResolvedValue({
    status: "error",
    error: commandError("NOT_FOUND", "missing"),
  });
  vi.mocked(commands.listInstruments).mockResolvedValue({ status: "ok", data: [] });
  vi.mocked(commands.listHoldings).mockResolvedValue({ status: "ok", data: [] });
  vi.mocked(commands.listAccountCash).mockResolvedValue({ status: "ok", data: [] });
  vi.mocked(commands.listInstrumentQuotes).mockResolvedValue({
    status: "ok",
    data: [],
  });
  vi.mocked(commands.listFxQuotes).mockResolvedValue({ status: "ok", data: [] });
  vi.mocked(commands.listRequiredFx).mockResolvedValue({ status: "ok", data: [] });
  vi.mocked(commands.getPortfolio).mockResolvedValue({
    status: "ok",
    data: emptyPortfolio(),
  });
  vi.mocked(commands.refreshAll).mockResolvedValue({
    status: "ok",
    data: { items: [] },
  });
  vi.mocked(commands.getHistoryOrigin).mockResolvedValue({
    status: "ok",
    data: confirmedHistoryOrigin(),
  });
  vi.mocked(commands.listActivities).mockResolvedValue({
    status: "ok",
    data: emptyActivityPage(),
  });
  vi.mocked(commands.getAccountTimeline).mockResolvedValue({
    status: "ok",
    data: emptyTimelinePage(),
  });
  vi.mocked(commands.getHistoryStatus).mockResolvedValue({
    status: "ok",
    data: emptyHistoryStatus(),
  });
  vi.mocked(commands.getNetWorthTrend).mockResolvedValue({
    status: "ok",
    data: emptyNetWorthTrend(),
  });
  vi.mocked(commands.rebuildHistorySnapshots).mockResolvedValue({
    status: "ok",
    data: emptyRebuildResult(),
  });
  vi.mocked(commands.getAnalyticsStatus).mockResolvedValue({
    status: "ok",
    data: emptyAnalyticsStatus(),
  });
  vi.mocked(commands.getPerformanceSummary).mockResolvedValue({
    status: "ok",
    data: emptyPerformanceSummary(),
  });
  vi.mocked(commands.getGainSummary).mockResolvedValue({
    status: "ok",
    data: emptyGainSummary(),
  });
  vi.mocked(commands.listHoldingGainSummaries).mockResolvedValue({
    status: "ok",
    data: { items: [] },
  });
  vi.mocked(commands.getNetWorthAttribution).mockResolvedValue({
    status: "ok",
    data: emptyAttribution(),
  });
  vi.mocked(commands.listHoldingLots).mockResolvedValue({
    status: "ok",
    data: emptyLotPage(),
  });
  vi.mocked(commands.listUnknownBasisLots).mockResolvedValue({
    status: "ok",
    data: emptyLotPage(),
  });
  vi.mocked(commands.listCostBasisDeclarations).mockResolvedValue({
    status: "ok",
    data: emptyDeclarationPage(),
  });
  vi.mocked(commands.declareLotCostBasis).mockResolvedValue({
    status: "error",
    error: commandError("COST_BASIS_LOT_NOT_FOUND", "missing"),
  });
  vi.mocked(commands.revokeLotCostBasis).mockResolvedValue({
    status: "error",
    error: commandError("COST_BASIS_LOT_NOT_FOUND", "missing"),
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
