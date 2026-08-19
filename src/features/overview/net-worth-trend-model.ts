import type { NetWorthTrendPointDto } from "@/generated/tauri-bindings";

export const TREND_RANGES = ["1m", "3m", "1y", "all"] as const;

export type TrendRange = (typeof TREND_RANGES)[number];

export type TrendPointState =
  "complete" | "incomplete" | "dirty" | "rebuilding" | "live" | "live-incomplete";

const PRESENTATION_SCALE = 8;

/**
 * Maps a canonical Money amount string to a presentation-only integer.
 * This is SVG/CSS geometry, not a financial calculation: displayed values must
 * keep using the original DTO strings.
 */
export function moneyPresentationUnits(amount: string): bigint {
  const negative = amount.startsWith("-");
  const unsigned = negative ? amount.slice(1) : amount;
  const [whole = "0", fraction = ""] = unsigned.split(".");
  const padded = (fraction + "0".repeat(PRESENTATION_SCALE)).slice(
    0,
    PRESENTATION_SCALE,
  );
  const units =
    BigInt(whole || "0") * 10n ** BigInt(PRESENTATION_SCALE) + BigInt(padded || "0");
  return negative ? -units : units;
}

export function chartDate(point: NetWorthTrendPointDto): string {
  if (point.date) {
    return point.date;
  }
  return point.asOf.slice(0, 10);
}

export function dateOrdinal(date: string): number {
  const match = /^(\d{4})-(\d{2})-(\d{2})$/.exec(date);
  if (!match) {
    return 0;
  }
  return (
    Date.UTC(Number(match[1]), Number(match[2]) - 1, Number(match[3])) / 86_400_000
  );
}

export function trendPointState(
  point: NetWorthTrendPointDto,
  dirtyFrom: string | null,
  rebuildStatus: string,
): TrendPointState {
  if (point.isLive) {
    return point.isComplete ? "live" : "live-incomplete";
  }
  const dirty = Boolean(dirtyFrom && point.date && point.date >= dirtyFrom);
  if (dirty && rebuildStatus === "running") {
    return "rebuilding";
  }
  if (dirty) {
    return "dirty";
  }
  if (!point.isComplete) {
    return "incomplete";
  }
  return "complete";
}

export function hasClosedDirtyDays(
  dirtyFrom: string | null | undefined,
  lastClosedOn: string | null | undefined,
): boolean {
  return Boolean(dirtyFrom && lastClosedOn && dirtyFrom <= lastClosedOn);
}

export function isTrustedComplete(state: TrendPointState): boolean {
  return state === "complete" || state === "live";
}

export function trendRows(
  points: NetWorthTrendPointDto[],
  current: NetWorthTrendPointDto,
): NetWorthTrendPointDto[] {
  return [...points, current];
}

export function pointKey(point: NetWorthTrendPointDto, index: number): string {
  if (point.isLive) {
    return `live:${point.asOf}`;
  }
  return `${point.date ?? "point"}:${index}`;
}
