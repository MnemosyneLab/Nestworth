import type { QueryClient } from "@tanstack/react-query";

import { moneyPresentationUnits } from "@/features/overview/net-worth-trend-model";
import type {
  AvailableAttributionDto,
  CostBasisDeclarationIpcDto,
  HoldingLotDto,
  LotRefDto,
  SignedMoneyDto,
} from "@/generated/tauri-bindings";

export const PAGE_SIZE = 50;

export const ATTRIBUTION_COMPONENT_KEYS = [
  "externalContributions",
  "externalWithdrawals",
  "instrumentMovement",
  "currencyMovement",
  "income",
  "fees",
  "debtPrincipalMovement",
  "conversionSpread",
  "unknownBasisFlow",
  "unexplained",
] as const;

export type AttributionComponentKey = (typeof ATTRIBUTION_COMPONENT_KEYS)[number];

export type ChartBar = {
  x: string;
  y: string;
  width: string;
  height: string;
};

export type BarGeometry = {
  viewBox: string;
  zeroY: string;
  width: string;
  height: string;
  bars: ChartBar[];
};

export async function invalidateAnalyticsQueries(queryClient: QueryClient) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["analytics-status"] }),
    queryClient.invalidateQueries({ queryKey: ["gain-summary"] }),
    queryClient.invalidateQueries({ queryKey: ["holding-gains"] }),
    queryClient.invalidateQueries({ queryKey: ["holding-lots"] }),
    queryClient.invalidateQueries({ queryKey: ["unknown-basis-lots"] }),
    queryClient.invalidateQueries({ queryKey: ["cost-basis-declarations"] }),
  ]);
}

export function lotKey(lot: { lotRef: LotRefDto; accountId: string }): string {
  return `${lot.lotRef.sourceKind}:${lot.lotRef.sourceId}:${lot.accountId}`;
}

export function declarationKey(item: CostBasisDeclarationIpcDto): string {
  return item.id;
}

export function mergeByLotRef(
  previous: HoldingLotDto[],
  incoming: HoldingLotDto[],
): HoldingLotDto[] {
  const keys = new Set(previous.map(lotKey));
  return [...previous, ...incoming.filter((item) => !keys.has(lotKey(item)))];
}

export function mergeByDeclarationId(
  previous: CostBasisDeclarationIpcDto[],
  incoming: CostBasisDeclarationIpcDto[],
): CostBasisDeclarationIpcDto[] {
  const keys = new Set(previous.map(declarationKey));
  return [
    ...previous,
    ...incoming.filter((item) => !keys.has(declarationKey(item))),
  ];
}

export function attributionComponents(
  value: AvailableAttributionDto,
): Array<{ key: AttributionComponentKey; money: SignedMoneyDto }> {
  return ATTRIBUTION_COMPONENT_KEYS.map((key) => ({
    key,
    money: value[key],
  }));
}

/**
 * Maps DTO amount strings onto integer SVG coordinates.
 * Displayed financial values must keep using the original strings.
 */
export function barGeometry(amounts: string[]): BarGeometry {
  const width = 640n;
  const height = 220n;
  const padLeft = 16n;
  const padRight = 16n;
  const padTop = 20n;
  const padBottom = 24n;
  const innerWidth = width - padLeft - padRight;
  const innerHeight = height - padTop - padBottom;
  const units = amounts.map((amount) => moneyPresentationUnits(amount));
  let min = 0n;
  let max = 0n;
  for (const value of units) {
    if (value < min) {
      min = value;
    }
    if (value > max) {
      max = value;
    }
  }
  const span = max - min;
  const count = units.length === 0 ? 1n : BigInt(units.length);
  const slot = innerWidth / count;
  const gap = slot / 6n;
  const barWidthRaw = slot - gap * 2n;
  const barWidth = barWidthRaw < 1n ? 1n : barWidthRaw;

  function yOf(value: bigint): bigint {
    if (span === 0n) {
      return padTop + innerHeight / 2n;
    }
    return padTop + ((max - value) * innerHeight) / span;
  }

  const zeroY = yOf(0n);
  const bars = units.map((value, index) => {
    const x = padLeft + BigInt(index) * slot + gap;
    const yValue = yOf(value);
    const top = value < 0n ? zeroY : yValue;
    const bottom = value < 0n ? yValue : zeroY;
    const barHeight = bottom === top ? 1n : bottom - top;
    return {
      x: x.toString(),
      y: top.toString(),
      width: barWidth.toString(),
      height: barHeight.toString(),
    };
  });
  return {
    viewBox: `0 0 ${width.toString()} ${height.toString()}`,
    zeroY: zeroY.toString(),
    width: width.toString(),
    height: height.toString(),
    bars,
  };
}
