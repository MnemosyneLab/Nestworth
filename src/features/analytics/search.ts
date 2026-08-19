import type {
  AnalyticsPeriodDto,
  AnalyticsScopeDto,
} from "@/generated/tauri-bindings";

export const ANALYTICS_SCOPES = [
  "household",
  "portfolio",
  "account",
  "instrument",
] as const;

export const ANALYTICS_PERIODS = [
  "oneMonth",
  "threeMonths",
  "oneYear",
  "all",
  "custom",
] as const;

export type AnalyticsScopeKind = (typeof ANALYTICS_SCOPES)[number];
export type AnalyticsPeriodKind = (typeof ANALYTICS_PERIODS)[number];

export type AnalyticsSearch = {
  scope?: AnalyticsScopeKind;
  accountId?: string;
  instrumentId?: string;
  period?: AnalyticsPeriodKind;
  start?: string;
  end?: string;
  lotCursor?: string;
  worklistCursor?: string;
  declarationCursor?: string;
};

const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

export function validateAnalyticsSearch(
  search: Record<string, unknown>,
): AnalyticsSearch {
  const next: AnalyticsSearch = {};
  const scope = readEnum(search.scope, ANALYTICS_SCOPES);
  const period = readEnum(search.period, ANALYTICS_PERIODS);
  const accountId = readString(search.accountId);
  const instrumentId = readString(search.instrumentId);
  const start = readDate(search.start);
  const end = readDate(search.end);
  const lotCursor = readString(search.lotCursor);
  const worklistCursor = readString(search.worklistCursor);
  const declarationCursor = readString(search.declarationCursor);
  if (scope) {
    next.scope = scope;
  }
  if (accountId) {
    next.accountId = accountId;
  }
  if (instrumentId) {
    next.instrumentId = instrumentId;
  }
  if (period) {
    next.period = period;
  }
  if (start) {
    next.start = start;
  }
  if (end) {
    next.end = end;
  }
  if (lotCursor) {
    next.lotCursor = lotCursor;
  }
  if (worklistCursor) {
    next.worklistCursor = worklistCursor;
  }
  if (declarationCursor) {
    next.declarationCursor = declarationCursor;
  }
  return next;
}

export function mergeAnalyticsSearch(
  prev: AnalyticsSearch | Record<string, unknown>,
  patch: Partial<AnalyticsSearch>,
): AnalyticsSearch {
  return validateAnalyticsSearch({ ...prev, ...patch });
}

export function resolvedScope(search: AnalyticsSearch): AnalyticsScopeKind {
  return search.scope ?? "household";
}

export function resolvedPeriod(search: AnalyticsSearch): AnalyticsPeriodKind {
  return search.period ?? "oneMonth";
}

export function isScopeReady(search: AnalyticsSearch): boolean {
  const scope = resolvedScope(search);
  if (scope === "account") {
    return Boolean(search.accountId);
  }
  if (scope === "instrument") {
    return Boolean(search.instrumentId);
  }
  return true;
}

export function toScopeDto(search: AnalyticsSearch): AnalyticsScopeDto {
  const scope = resolvedScope(search);
  if (scope === "account" && search.accountId) {
    return { kind: "account", accountId: search.accountId };
  }
  if (scope === "instrument" && search.instrumentId) {
    return { kind: "instrument", instrumentId: search.instrumentId };
  }
  if (scope === "portfolio") {
    return { kind: "portfolio" };
  }
  return { kind: "household" };
}

export function toPeriodDto(search: AnalyticsSearch): AnalyticsPeriodDto | null {
  const period = resolvedPeriod(search);
  if (period === "custom") {
    if (!search.start || !search.end) {
      return null;
    }
    return {
      kind: "custom",
      startLocalDate: search.start,
      endLocalDate: search.end,
    };
  }
  return { kind: period };
}

export function lotInstrumentId(search: AnalyticsSearch): string | undefined {
  if (resolvedScope(search) === "instrument") {
    return search.instrumentId;
  }
  return search.instrumentId;
}

function readString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}

function readDate(value: unknown): string | undefined {
  const text = readString(value);
  return text && DATE_PATTERN.test(text) ? text : undefined;
}

function readEnum<T extends string>(
  value: unknown,
  allowed: readonly T[],
): T | undefined {
  const text = readString(value);
  return text && (allowed as readonly string[]).includes(text)
    ? (text as T)
    : undefined;
}
