import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState, type KeyboardEvent } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  TREND_RANGES,
  chartDate,
  dateOrdinal,
  hasClosedDirtyDays,
  isTrustedComplete,
  moneyPresentationUnits,
  pointKey,
  trendPointState,
  trendRows,
  type TrendPointState,
  type TrendRange,
} from "@/features/overview/net-worth-trend-model";
import type {
  HistoryStatusDto,
  NetWorthTrendDto,
  NetWorthTrendPointDto,
  RebuildHistorySnapshotsResultDto,
  ReferenceCatalogDto,
} from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import { formatReferenceMoney } from "@/lib/reference-catalog";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { cn } from "@/lib/utils";

export function NetWorthTrendSection({ catalog }: { catalog: ReferenceCatalogDto }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [range, setRange] = useState<TrendRange>("1m");
  const [progress, setProgress] = useState<RebuildHistorySnapshotsResultDto | null>(
    null,
  );
  const cancelRequested = useRef(false);

  const statusQuery = useQuery({
    queryKey: ["history-status"],
    queryFn: () => unwrapResult(commands.getHistoryStatus()),
  });
  const trendQuery = useQuery({
    queryKey: ["net-worth-trend", range],
    queryFn: () => unwrapResult(commands.getNetWorthTrend({ range })),
  });

  async function invalidateHistory() {
    await Promise.all([
      queryClient.invalidateQueries({ queryKey: ["history-status"] }),
      queryClient.invalidateQueries({ queryKey: ["net-worth-trend"] }),
    ]);
  }

  const rebuild = useMutation({
    mutationFn: async () => {
      cancelRequested.current = false;
      setProgress(null);
      let latest = await unwrapResult(commands.rebuildHistorySnapshots({}));
      if (cancelRequested.current) {
        return latest;
      }
      setProgress(latest);
      while (
        latest.remaining &&
        latest.processedDays > 0 &&
        !latest.cancelled &&
        !cancelRequested.current
      ) {
        latest = await unwrapResult(commands.rebuildHistorySnapshots({}));
        if (cancelRequested.current) {
          return latest;
        }
        setProgress(latest);
      }
      return latest;
    },
    onSuccess: async () => {
      await invalidateHistory();
    },
  });
  const cancelRebuild = useMutation({
    mutationFn: async () => {
      cancelRequested.current = true;
      return unwrapResult(commands.rebuildHistorySnapshots({ cancel: true }));
    },
    onSuccess: async (result) => {
      setProgress(result);
      await invalidateHistory();
    },
  });

  const error = trendQuery.error
    ? commandErrorFromUnknown(trendQuery.error)
    : statusQuery.error
      ? commandErrorFromUnknown(statusQuery.error)
      : rebuild.error
        ? commandErrorFromUnknown(rebuild.error)
        : cancelRebuild.error
          ? commandErrorFromUnknown(cancelRebuild.error)
          : null;
  const status = statusQuery.data;
  const trend = trendQuery.data;
  const rebuilding =
    !progress?.cancelled &&
    (rebuild.isPending ||
      cancelRebuild.isPending ||
      status?.rebuildStatus === "running");

  return (
    <section className="space-y-4">
      <div className="flex flex-wrap items-end justify-between gap-3">
        <h2 className="text-lg font-medium">{t("overview.trend.title")}</h2>
        <RangeControl range={range} onChange={setRange} />
      </div>
      {trendQuery.isPending || statusQuery.isPending ? (
        <p role="status">{t("references.loading")}</p>
      ) : null}
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, error)}
        </p>
      ) : null}
      {status ? (
        <RebuildPanel
          lastResult={progress}
          rebuilding={rebuilding}
          status={status}
          onCancel={() => cancelRebuild.mutate()}
          onContinue={() => rebuild.mutate()}
          onRebuild={() => rebuild.mutate()}
          onRetry={() => rebuild.mutate()}
        />
      ) : null}
      {trend ? <TrendBody catalog={catalog} status={status} trend={trend} /> : null}
    </section>
  );
}

function RangeControl({
  onChange,
  range,
}: {
  onChange: (range: TrendRange) => void;
  range: TrendRange;
}) {
  const { t } = useTranslation();

  function onKeyDown(event: KeyboardEvent<HTMLDivElement>) {
    const index = TREND_RANGES.indexOf(range);
    if (event.key === "ArrowRight" || event.key === "ArrowDown") {
      event.preventDefault();
      onChange(TREND_RANGES[(index + 1) % TREND_RANGES.length]);
    }
    if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
      event.preventDefault();
      onChange(TREND_RANGES[(index - 1 + TREND_RANGES.length) % TREND_RANGES.length]);
    }
  }

  return (
    <div
      aria-label={t("overview.trend.range")}
      className="flex flex-wrap gap-1"
      onKeyDown={onKeyDown}
      role="radiogroup"
    >
      {TREND_RANGES.map((value) => {
        const selected = value === range;
        return (
          <Button
            aria-checked={selected}
            className="h-9 px-3"
            key={value}
            onClick={() => onChange(value)}
            role="radio"
            tabIndex={selected ? 0 : -1}
            type="button"
            variant={selected ? "primary" : "ghost"}
          >
            {t(`overview.trend.ranges.${value}`)}
          </Button>
        );
      })}
    </div>
  );
}

function RebuildPanel({
  lastResult,
  onCancel,
  onContinue,
  onRebuild,
  onRetry,
  rebuilding,
  status,
}: {
  lastResult: RebuildHistorySnapshotsResultDto | null;
  onCancel: () => void;
  onContinue: () => void;
  onRebuild: () => void;
  onRetry: () => void;
  rebuilding: boolean;
  status: HistoryStatusDto;
}) {
  const { t } = useTranslation();
  const dirtyFrom = lastResult?.dirtyFrom ?? status.dirtyFrom;
  const lastCompletedOn = lastResult?.lastCompletedOn ?? status.lastCompletedOn;
  const failed = status.rebuildStatus === "failed";
  const cancelled = lastResult?.cancelled === true;
  const remaining = lastResult
    ? lastResult.remaining
    : hasClosedDirtyDays(dirtyFrom, status.lastClosedOn);

  if (!remaining && !rebuilding && !failed) {
    return null;
  }

  return (
    <div className="space-y-3 rounded-xl border border-border bg-card px-4 py-4">
      <p className="text-sm text-muted-foreground">
        {t("overview.trend.rebuildLocal")}
      </p>
      {rebuilding ? (
        <p role="status">
          {lastCompletedOn || lastResult?.processedDays
            ? t("overview.trend.rebuildProgress", {
                processed: lastResult?.processedDays ?? 0,
                date: lastCompletedOn ?? t("overview.trend.rebuildProgressUnknown"),
              })
            : t("overview.trend.rebuildProgressUnknown")}
        </p>
      ) : null}
      {!rebuilding && remaining && dirtyFrom ? (
        <p>
          {t("overview.trend.rebuildPrompt", { date: dirtyFrom })}
          {lastCompletedOn
            ? ` ${t("overview.trend.lastCompleted", { date: lastCompletedOn })}`
            : ""}
        </p>
      ) : null}
      {!rebuilding && cancelled && remaining && dirtyFrom ? (
        <p role="status">{t("overview.trend.rebuildCancelled", { date: dirtyFrom })}</p>
      ) : null}
      {!rebuilding && remaining && lastCompletedOn && !cancelled ? (
        <p>{t("overview.trend.rebuildRemaining")}</p>
      ) : null}
      <div className="flex flex-wrap gap-2">
        {rebuilding ? (
          <Button onClick={onCancel} type="button" variant="ghost">
            {t("overview.trend.rebuildCancel")}
          </Button>
        ) : failed ? (
          <Button onClick={onRetry} type="button">
            {t("overview.trend.rebuildRetry")}
          </Button>
        ) : remaining && lastCompletedOn ? (
          <Button onClick={onContinue} type="button">
            {t("overview.trend.rebuildContinue")}
          </Button>
        ) : remaining ? (
          <Button onClick={onRebuild} type="button">
            {t("overview.trend.rebuild")}
          </Button>
        ) : null}
      </div>
    </div>
  );
}

function TrendBody({
  catalog,
  status,
  trend,
}: {
  catalog: ReferenceCatalogDto;
  status: HistoryStatusDto | undefined;
  trend: NetWorthTrendDto;
}) {
  const { t } = useTranslation();
  const dirtyFrom = trend.dirtyFrom ?? status?.dirtyFrom ?? null;
  const rebuildStatus = status?.rebuildStatus ?? "idle";
  const rows = trendRows(trend.points, trend.current);
  const emptyHistory = trend.points.length === 0;

  return (
    <div className="space-y-4">
      <p className="text-sm text-muted-foreground">
        {t("overview.trend.preOrigin", { date: trend.originLocalDate })}
      </p>
      {emptyHistory ? (
        <p className="text-sm text-muted-foreground">
          {t("overview.trend.emptyRange")}
        </p>
      ) : null}
      <TrendChart dirtyFrom={dirtyFrom} points={rows} rebuildStatus={rebuildStatus} />
      <TrendTable
        catalog={catalog}
        dirtyFrom={dirtyFrom}
        points={rows}
        rebuildStatus={rebuildStatus}
      />
    </div>
  );
}

function TrendChart({
  dirtyFrom,
  points,
  rebuildStatus,
}: {
  dirtyFrom: string | null;
  points: NetWorthTrendPointDto[];
  rebuildStatus: string;
}) {
  const { t } = useTranslation();
  const width = 640;
  const height = 220;
  const pad = { left: 16, right: 16, top: 20, bottom: 20 };
  const innerWidth = width - pad.left - pad.right;
  const innerHeight = height - pad.top - pad.bottom;
  const states = points.map((point) =>
    trendPointState(point, dirtyFrom, rebuildStatus),
  );
  const xs = points.map((point) => dateOrdinal(chartDate(point)));
  const ys = points.map((point) => moneyPresentationUnits(point.netWorth.amount));
  const minX = xs.length === 0 ? 0 : Math.min(...xs);
  const maxX = xs.length === 0 ? 0 : Math.max(...xs);
  const minY =
    ys.length === 0 ? 0n : ys.reduce((min, value) => (value < min ? value : min));
  const maxY =
    ys.length === 0 ? 0n : ys.reduce((max, value) => (value > max ? value : max));

  function xAt(index: number): number {
    if (xs.length <= 1 || minX === maxX) {
      return pad.left + innerWidth / 2;
    }
    return pad.left + ((xs[index] - minX) / (maxX - minX)) * innerWidth;
  }

  function yAt(index: number): number {
    if (ys.length <= 1 || minY === maxY) {
      return pad.top + innerHeight / 2;
    }
    const ratio = Number(ys[index] - minY) / Number(maxY - minY);
    return pad.top + innerHeight - ratio * innerHeight;
  }

  const segments = [];
  for (let index = 1; index < points.length; index += 1) {
    const trusted =
      isTrustedComplete(states[index - 1]) && isTrustedComplete(states[index]);
    segments.push({
      key: `${pointKey(points[index - 1], index - 1)}:${pointKey(points[index], index)}`,
      trusted,
      d: `M ${xAt(index - 1)} ${yAt(index - 1)} L ${xAt(index)} ${yAt(index)}`,
    });
  }

  return (
    <svg
      aria-hidden="true"
      className="h-56 w-full"
      role="presentation"
      viewBox={`0 0 ${width} ${height}`}
    >
      <title>{t("overview.trend.chart")}</title>
      {segments.map((segment) => (
        <path
          d={segment.d}
          data-segment={segment.trusted ? "trusted" : "distinct"}
          fill="none"
          key={segment.key}
          stroke="currentColor"
          strokeDasharray={segment.trusted ? undefined : "6 6"}
          strokeWidth="2"
          className={segment.trusted ? "text-primary" : "text-muted-foreground"}
        />
      ))}
      {points.map((point, index) => {
        const state = states[index];
        return (
          <circle
            className={markerClass(state)}
            cx={xAt(index)}
            cy={yAt(index)}
            data-trend-state={state}
            fill={isTrustedComplete(state) ? "currentColor" : "none"}
            key={pointKey(point, index)}
            r={point.isLive ? 6 : 4}
            stroke="currentColor"
            strokeDasharray={isTrustedComplete(state) ? undefined : "3 3"}
            strokeWidth="2"
          />
        );
      })}
    </svg>
  );
}

function TrendTable({
  catalog,
  dirtyFrom,
  points,
  rebuildStatus,
}: {
  catalog: ReferenceCatalogDto;
  dirtyFrom: string | null;
  points: NetWorthTrendPointDto[];
  rebuildStatus: string;
}) {
  const { t } = useTranslation();
  return (
    <div className="overflow-x-auto">
      <table
        aria-label={t("overview.trend.table")}
        className="w-full min-w-[36rem] border-collapse text-left text-sm"
      >
        <thead>
          <tr>
            <th className="border-b border-border py-2 pr-3 font-medium" scope="col">
              {t("overview.trend.date")}
            </th>
            <th className="border-b border-border py-2 pr-3 font-medium" scope="col">
              {t("overview.trend.value")}
            </th>
            <th className="border-b border-border py-2 pr-3 font-medium" scope="col">
              {t("overview.trend.currency")}
            </th>
            <th className="border-b border-border py-2 pr-3 font-medium" scope="col">
              {t("overview.trend.completeness")}
            </th>
            <th className="border-b border-border py-2 font-medium" scope="col">
              {t("overview.trend.missingCount")}
            </th>
          </tr>
        </thead>
        <tbody>
          {points.map((point, index) => {
            const state = trendPointState(point, dirtyFrom, rebuildStatus);
            const dateLabel = point.isLive
              ? t("overview.trend.current")
              : (point.date ?? chartDate(point));
            const formatted = formatReferenceMoney(
              t,
              catalog,
              point.netWorth.amount,
              point.netWorth.currency,
            );
            const completeness = completenessLabel(t, state, point.missingCount);
            return (
              <tr
                aria-label={t("overview.trend.pointLabel", {
                  date: dateLabel,
                  value: formatted,
                  currency: point.netWorth.currency,
                  completeness,
                })}
                className={cn(
                  "border-b border-border",
                  state === "incomplete" || state === "live-incomplete"
                    ? "text-destructive"
                    : state === "dirty" || state === "rebuilding"
                      ? "text-muted-foreground"
                      : undefined,
                )}
                data-trend-state={state}
                key={pointKey(point, index)}
                tabIndex={0}
              >
                <td className="py-2 pr-3">{dateLabel}</td>
                <td className="py-2 pr-3">{formatted}</td>
                <td className="py-2 pr-3">{point.netWorth.currency}</td>
                <td className="py-2 pr-3">{completeness}</td>
                <td className="py-2">{point.missingCount}</td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function completenessLabel(
  t: ReturnType<typeof useTranslation>["t"],
  state: TrendPointState,
  missingCount: number,
): string {
  if (state === "incomplete" || state === "live-incomplete") {
    return t("overview.trend.incompleteMissing", { count: missingCount });
  }
  return t(`overview.trend.states.${state}`);
}

function markerClass(state: TrendPointState): string {
  if (state === "incomplete" || state === "live-incomplete") {
    return "text-destructive";
  }
  if (state === "dirty" || state === "rebuilding") {
    return "text-warning";
  }
  if (state === "live") {
    return "text-primary";
  }
  return "text-primary";
}
