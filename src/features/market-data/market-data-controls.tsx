import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  commands,
  type CommandError,
  type RefreshResultDto,
} from "@/generated/tauri-bindings";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { invalidateMarketData } from "@/lib/tauri/invalidate";

const MAX_HISTORY_DAYS = 3660;

export function useMarketDataCapabilitiesQuery() {
  return useQuery({
    queryKey: ["market-data-capabilities"],
    queryFn: () => unwrapResult(commands.getMarketDataCapabilities()),
    retry: false,
    staleTime: Infinity,
  });
}

export function InstrumentMarketDataControls({
  instrumentId,
  providerReady,
}: {
  instrumentId: string;
  providerReady: boolean;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [result, setResult] = useState<RefreshResultDto | null>(null);
  const [error, setError] = useState<CommandError | null>(null);
  const current = useMutation({
    mutationFn: () => unwrapResult(commands.refreshInstrument({ instrumentId })),
    onSuccess: async (value) => {
      setError(null);
      setResult(value);
      await invalidateMarketData(queryClient, instrumentId);
    },
    onError: (value) => setError(commandErrorFromUnknown(value)),
  });

  if (!providerReady) {
    return null;
  }

  return (
    <div className="space-y-3 rounded-lg border border-border px-3 py-3">
      <div>
        <h3 className="text-sm font-medium">{t("marketData.title")}</h3>
        <p className="mt-1 text-sm text-muted-foreground">
          {t("marketData.instrumentHelp")}
        </p>
      </div>
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, error)}
        </p>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <Button
          disabled={current.isPending}
          onClick={() => {
            setError(null);
            current.mutate();
          }}
          type="button"
        >
          {current.isPending
            ? t("marketData.refreshing")
            : t("marketData.refreshCurrent")}
        </Button>
      </div>
      <HistoryBackfillForm
        forceLabel={t("marketData.forceRefetch")}
        onComplete={(value) => setResult(value)}
        onError={(value) => setError(value)}
        target={{ kind: "instrument", instrumentId }}
      />
      {result ? <RefreshResultSummary result={result} /> : null}
    </div>
  );
}

export function MarketDataBulkControls({
  kind,
  disabled,
}: {
  kind: "all" | "fx";
  disabled?: boolean;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [result, setResult] = useState<RefreshResultDto | null>(null);
  const [error, setError] = useState<CommandError | null>(null);
  const [lastInput, setLastInput] = useState<{
    startLocalDate: string;
    endLocalDate: string;
    force: boolean;
  } | null>(null);
  const mutation = useMutation({
    mutationFn: (input: {
      startLocalDate: string;
      endLocalDate: string;
      force: boolean;
    }) =>
      unwrapResult(
        kind === "all"
          ? commands.backfillAllHistory(input)
          : commands.backfillRequiredFxHistory(input),
      ),
    onSuccess: async (value) => {
      setError(null);
      setResult(value);
      await invalidateMarketData(queryClient);
    },
    onError: (value) => setError(commandErrorFromUnknown(value)),
  });

  return (
    <section className="space-y-3 rounded-xl border border-border bg-card px-4 py-4">
      <div>
        <h3 className="font-medium">{t("marketData.bulkTitle")}</h3>
        <p className="mt-1 text-sm text-muted-foreground">
          {t(kind === "all" ? "marketData.bulkAllHelp" : "marketData.bulkFxHelp")}
        </p>
      </div>
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, error)}
        </p>
      ) : null}
      <HistoryBackfillForm
        disabled={disabled || mutation.isPending}
        forceLabel={t("marketData.forceRefetch")}
        onComplete={(value) => {
          setError(null);
          setResult(value);
        }}
        onError={(value) => setError(value)}
        target={{ kind }}
        onSubmit={(input) => {
          setLastInput(input);
          return mutation.mutateAsync(input);
        }}
      />
      {result ? (
        <RefreshResultSummary
          onRetry={
            lastInput && result.items.some((item) => item.status === "failed")
              ? () => mutation.mutate(lastInput)
              : undefined
          }
          retrying={mutation.isPending}
          result={result}
        />
      ) : null}
    </section>
  );
}

type BackfillTarget =
  { kind: "instrument"; instrumentId: string } | { kind: "fx" } | { kind: "all" };

function HistoryBackfillForm({
  disabled,
  forceLabel,
  onComplete,
  onError,
  onSubmit,
  target,
}: {
  disabled?: boolean;
  forceLabel: string;
  onComplete: (result: RefreshResultDto) => void;
  onError: (error: CommandError) => void;
  onSubmit?: (input: {
    startLocalDate: string;
    endLocalDate: string;
    force: boolean;
  }) => Promise<RefreshResultDto>;
  target: BackfillTarget;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const history = useQuery({
    queryKey: ["history-status"],
    queryFn: () => unwrapResult(commands.getHistoryStatus()),
  });
  const [startLocalDate, setStartLocalDate] = useState("");
  const [endLocalDate, setEndLocalDate] = useState("");
  const [force, setForce] = useState(false);
  const [normalCompleted, setNormalCompleted] = useState(false);
  const mutation = useMutation({
    mutationFn: (input: {
      startLocalDate: string;
      endLocalDate: string;
      force: boolean;
    }) => {
      if (onSubmit) {
        return onSubmit(input);
      }
      if (target.kind === "instrument") {
        return unwrapResult(
          commands.backfillInstrumentHistory({
            instrumentId: target.instrumentId,
            ...input,
          }),
        );
      }
      return unwrapResult(commands.backfillRequiredFxHistory(input));
    },
    onSuccess: async (value) => {
      if (!value) {
        return;
      }
      onComplete(value);
      if (!force) {
        setNormalCompleted(true);
      }
      await invalidateMarketData(
        queryClient,
        target.kind === "instrument" ? target.instrumentId : undefined,
      );
    },
    onError,
  });
  const lastClosedOn = history.data?.lastClosedOn ?? "";
  const defaultStart = lastClosedOn;
  const defaultEnd = lastClosedOn;

  function submit() {
    const start = startLocalDate || defaultStart;
    const end = endLocalDate || defaultEnd;
    if (!start || !end) {
      onError({
        code: "MARKET_DATA_HISTORY_INVALID_RANGE",
        message: t("marketData.rangeRequired"),
        fields: null,
      });
      return;
    }
    if (target.kind !== "instrument" && !window.confirm(t("marketData.bulkConfirm"))) {
      return;
    }
    mutation.mutate({ startLocalDate: start, endLocalDate: end, force });
  }

  return (
    <div className="space-y-3 rounded-lg border border-border px-3 py-3">
      <div>
        <h4 className="text-sm font-medium">{t("marketData.backfillTitle")}</h4>
        <p className="mt-1 text-sm text-muted-foreground">
          {t("marketData.rangeHelp", { days: MAX_HISTORY_DAYS })}
        </p>
      </div>
      <div className="grid gap-3 sm:grid-cols-2">
        <label className="grid gap-1 text-sm">
          {t("marketData.startDate")}
          <Input
            disabled={disabled || history.isPending}
            max={lastClosedOn || undefined}
            onChange={(event) => setStartLocalDate(event.target.value)}
            type="date"
            value={startLocalDate || defaultStart}
          />
        </label>
        <label className="grid gap-1 text-sm">
          {t("marketData.endDate")}
          <Input
            disabled={disabled || history.isPending}
            max={lastClosedOn || undefined}
            onChange={(event) => setEndLocalDate(event.target.value)}
            type="date"
            value={endLocalDate || defaultEnd}
          />
        </label>
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <Button
          disabled={disabled || mutation.isPending || history.isPending}
          onClick={submit}
          type="button"
        >
          {mutation.isPending ? t("marketData.backfilling") : t("marketData.backfill")}
        </Button>
        {normalCompleted ? (
          <label className="flex items-center gap-2 text-sm">
            <input
              checked={force}
              disabled={disabled || mutation.isPending}
              onChange={(event) => setForce(event.target.checked)}
              type="checkbox"
            />
            {forceLabel}
          </label>
        ) : null}
      </div>
      <p className="text-xs text-muted-foreground">
        {history.isPending
          ? t("marketData.loadingHistoryStatus")
          : t("marketData.lastClosed", {
              date: lastClosedOn || t("marketData.unknown"),
            })}
      </p>
    </div>
  );
}

export function RefreshResultSummary({
  onRetry,
  result,
  retrying = false,
}: {
  onRetry?: () => void;
  result: RefreshResultDto;
  retrying?: boolean;
}) {
  const { t } = useTranslation();
  const failed = result.items.filter((item) => item.status === "failed").length;
  return (
    <div className="space-y-1 text-sm" role="status">
      <p>{t("marketData.resultSummary", { count: result.items.length, failed })}</p>
      <ul className="space-y-1 text-muted-foreground">
        {result.items.map((item) => (
          <li key={`${item.key}:${item.status}`}>
            {item.key}:{" "}
            {t(`marketData.status.${item.status}`, { defaultValue: item.status })}
            {item.errorCode
              ? ` · ${t(`errors.${item.errorCode}`, { defaultValue: item.errorCode })}`
              : ""}
          </li>
        ))}
      </ul>
      {onRetry ? (
        <Button disabled={retrying} onClick={onRetry} type="button" variant="ghost">
          {retrying ? t("marketData.retrying") : t("marketData.retryFailed")}
        </Button>
      ) : null}
    </div>
  );
}
