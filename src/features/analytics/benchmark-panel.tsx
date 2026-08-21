import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  commands,
  type AnalyticsPeriodDto,
  type AnalyticsScopeDto,
  type BenchmarkComparisonDto,
  type BenchmarkDto,
  type BenchmarkObservationDto,
  type ReferenceCatalogDto,
} from "@/generated/tauri-bindings";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";

export function BenchmarkPanel({
  catalog,
  period,
  scope,
  ready,
}: {
  catalog: ReferenceCatalogDto;
  period: AnalyticsPeriodDto | null;
  scope: AnalyticsScopeDto;
  ready: boolean;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [creating, setCreating] = useState(false);
  const [name, setName] = useState("");
  const [currency, setCurrency] = useState(catalog.currencies[0]?.value ?? "CNY");
  const [seriesKind, setSeriesKind] = useState("price_return");
  const [carryDays, setCarryDays] = useState("7");
  const [level, setLevel] = useState("");
  const [observedOn, setObservedOn] = useState("");
  const [actionError, setActionError] = useState<ReturnType<
    typeof commandErrorFromUnknown
  > | null>(null);

  const benchmarks = useQuery({
    queryKey: ["benchmarks", true],
    queryFn: () => unwrapResult(commands.listBenchmarks({ includeArchived: true })),
  });
  const selected =
    (benchmarks.data ?? []).find((benchmark) => benchmark.id === selectedId) ??
    (benchmarks.data ?? []).find((benchmark) => benchmark.isDefault) ??
    null;
  const observations = useQuery({
    queryKey: ["benchmark-observations", selected?.id],
    queryFn: () =>
      unwrapResult(commands.listBenchmarkObservations({ benchmarkId: selected!.id })),
    enabled: Boolean(selected),
  });
  const comparison = useQuery({
    queryKey: ["benchmark-comparison", scope, period, selected?.id],
    queryFn: () =>
      unwrapResult(
        commands.getBenchmarkComparison({
          scope,
          period: period!,
          benchmarkId: selected?.id ?? null,
        }),
      ),
    enabled: ready && Boolean(selected),
  });
  const mutate = useMutation({
    mutationFn: async (operation: () => Promise<unknown>) => operation(),
    onSuccess: async () => {
      setActionError(null);
      await queryClient.invalidateQueries({ queryKey: ["benchmarks"] });
      await queryClient.invalidateQueries({ queryKey: ["benchmark-observations"] });
      await queryClient.invalidateQueries({ queryKey: ["benchmark-comparison"] });
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  const error =
    actionError ??
    (benchmarks.error ? commandErrorFromUnknown(benchmarks.error) : null) ??
    (comparison.error ? commandErrorFromUnknown(comparison.error) : null);
  return (
    <section
      aria-labelledby="benchmark-panel-title"
      className="mt-8 space-y-4"
      id="benchmark"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <div>
          <h2 className="text-lg font-medium" id="benchmark-panel-title">
            {t("benchmarks.title")}
          </h2>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("benchmarks.description")}
          </p>
        </div>
        <Button
          onClick={() => setCreating((value) => !value)}
          type="button"
          variant="ghost"
        >
          {creating ? t("references.cancel") : t("benchmarks.new")}
        </Button>
      </div>
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, error)}
        </p>
      ) : null}
      {creating ? (
        <form
          className="grid gap-3 rounded-lg border border-border bg-card p-4 md:grid-cols-4"
          onSubmit={(event) => {
            event.preventDefault();
            mutate.mutate(() =>
              unwrapResult(
                commands.createBenchmark({
                  name,
                  currency,
                  seriesKind,
                  maxCarryDays: Number.parseInt(carryDays, 10),
                }),
              ),
            );
            setCreating(false);
          }}
        >
          <label className="grid gap-1 text-xs font-medium">
            {t("benchmarks.name")}
            <Input
              onChange={(event) => setName(event.target.value)}
              required
              value={name}
            />
          </label>
          <label className="grid gap-1 text-xs font-medium">
            {t("benchmarks.currency")}
            <select
              className={selectClass}
              onChange={(event) => setCurrency(event.target.value)}
              value={currency}
            >
              {catalog.currencies.map((option) => (
                <option key={option.value} value={option.value}>
                  {option.value}
                </option>
              ))}
            </select>
          </label>
          <label className="grid gap-1 text-xs font-medium">
            {t("benchmarks.seriesKind")}
            <select
              className={selectClass}
              onChange={(event) => setSeriesKind(event.target.value)}
              value={seriesKind}
            >
              <option value="price_return">{t("benchmarks.priceReturn")}</option>
              <option value="total_return">{t("benchmarks.totalReturn")}</option>
            </select>
          </label>
          <label className="grid gap-1 text-xs font-medium">
            {t("benchmarks.carryDays")}
            <Input
              min={0}
              max={3650}
              onChange={(event) => setCarryDays(event.target.value)}
              required
              type="number"
              value={carryDays}
            />
          </label>
          <div className="md:col-span-4">
            <Button disabled={mutate.isPending} type="submit">
              {t("references.save")}
            </Button>
          </div>
        </form>
      ) : null}
      {benchmarks.isPending ? <p role="status">{t("references.loading")}</p> : null}
      <div className="grid gap-2 md:grid-cols-2">
        {(benchmarks.data ?? []).map((benchmark) => (
          <BenchmarkCard
            benchmark={benchmark}
            key={benchmark.id}
            onArchive={() =>
              mutate.mutate(() =>
                unwrapResult(commands.archiveBenchmark({ id: benchmark.id })),
              )
            }
            onRestore={() =>
              mutate.mutate(() =>
                unwrapResult(commands.restoreBenchmark({ id: benchmark.id })),
              )
            }
            onSelect={() => setSelectedId(benchmark.id)}
            onSetDefault={() =>
              mutate.mutate(() =>
                unwrapResult(
                  commands.setDefaultBenchmark({ benchmarkId: benchmark.id }),
                ),
              )
            }
            selected={selected?.id === benchmark.id}
            t={t}
          />
        ))}
      </div>
      {selected ? (
        <BenchmarkDetails
          benchmark={selected}
          observations={observations.data ?? []}
          level={level}
          observedOn={observedOn}
          onAppend={() =>
            mutate.mutate(() =>
              unwrapResult(
                commands.appendBenchmarkObservation({
                  benchmarkId: selected.id,
                  level,
                  observedOn,
                  note: null,
                }),
              ),
            )
          }
          onLevelChange={setLevel}
          onObservedOnChange={setObservedOn}
          comparison={comparison.data ?? null}
          t={t}
        />
      ) : (
        <p className="text-sm text-muted-foreground">{t("benchmarks.empty")}</p>
      )}
    </section>
  );
}

function BenchmarkCard({
  benchmark,
  onArchive,
  onRestore,
  onSelect,
  onSetDefault,
  selected,
  t,
}: {
  benchmark: BenchmarkDto;
  onArchive: () => void;
  onRestore: () => void;
  onSelect: () => void;
  onSetDefault: () => void;
  selected: boolean;
  t: (key: string) => string;
}) {
  return (
    <article
      className={`rounded-lg border p-4 ${selected ? "border-primary bg-accent/40" : "border-border bg-card"}`}
    >
      <button className="block w-full text-left" onClick={onSelect} type="button">
        <h3 className="font-medium">{benchmark.name}</h3>
        <p className="mt-1 text-sm text-muted-foreground">
          {benchmark.currency} · {benchmark.seriesKind} · {benchmark.maxCarryDays} days
        </p>
      </button>
      <div className="mt-3 flex flex-wrap gap-2">
        {benchmark.isDefault ? (
          <span className="rounded-full bg-surface-soft px-2 py-1 text-xs">
            {t("benchmarks.default")}
          </span>
        ) : (
          <Button onClick={onSetDefault} type="button" variant="ghost">
            {t("benchmarks.setDefault")}
          </Button>
        )}
        {benchmark.archivedAt ? (
          <Button onClick={onRestore} type="button" variant="ghost">
            {t("references.restore")}
          </Button>
        ) : (
          <Button onClick={onArchive} type="button" variant="ghost">
            {t("references.archive")}
          </Button>
        )}
      </div>
    </article>
  );
}

function BenchmarkDetails({
  benchmark,
  observations,
  level,
  observedOn,
  onAppend,
  onLevelChange,
  onObservedOnChange,
  comparison,
  t,
}: {
  benchmark: BenchmarkDto;
  observations: BenchmarkObservationDto[];
  level: string;
  observedOn: string;
  onAppend: () => void;
  onLevelChange: (value: string) => void;
  onObservedOnChange: (value: string) => void;
  comparison: BenchmarkComparisonDto | null;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  return (
    <div className="rounded-lg border border-border bg-card p-4">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="font-medium">
            {t("benchmarks.observations")}: {benchmark.name}
          </h3>
          <p className="mt-1 text-sm text-muted-foreground">
            {benchmark.currency} · {benchmark.seriesKind}
          </p>
        </div>
        <div className="flex flex-wrap items-end gap-2">
          <label className="grid gap-1 text-xs font-medium">
            {t("benchmarks.level")}
            <Input
              onChange={(event) => onLevelChange(event.target.value)}
              value={level}
            />
          </label>
          <label className="grid gap-1 text-xs font-medium">
            {t("benchmarks.date")}
            <Input
              onChange={(event) => onObservedOnChange(event.target.value)}
              type="date"
              value={observedOn}
            />
          </label>
          <Button disabled={!level || !observedOn} onClick={onAppend} type="button">
            {t("benchmarks.append")}
          </Button>
        </div>
      </div>
      <ul className="mt-4 divide-y divide-border text-sm">
        {observations.map((observation) => (
          <li className="flex justify-between gap-2 py-2" key={observation.id}>
            <span>{observation.observedOn}</span>
            <span>
              {observation.level} {observation.sourceKind}
            </span>
          </li>
        ))}
      </ul>
      {comparison ? <Comparison comparison={comparison} t={t} /> : null}
    </div>
  );
}

function Comparison({
  comparison,
  t,
}: {
  comparison: BenchmarkComparisonDto;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const returnValue =
    comparison.benchmarkReturn.kind === "available"
      ? comparison.benchmarkReturn.cumulative
      : null;
  const excess =
    comparison.excessReturn.kind === "available"
      ? comparison.excessReturn.percentagePoints
      : null;
  const portfolio =
    comparison.portfolioTwr.kind === "available"
      ? comparison.portfolioTwr.cumulative
      : null;
  return (
    <div className="mt-5 border-t border-border pt-4">
      <h4 className="font-medium">{t("benchmarks.comparison")}</h4>
      <p className="mt-1 text-xs text-muted-foreground">
        {comparison.startOn} → {comparison.endOn}
      </p>
      <dl className="mt-3 grid gap-3 sm:grid-cols-3">
        <div>
          <dt className="text-xs text-muted-foreground">
            {t("benchmarks.portfolioTwr")}
          </dt>
          <dd className="text-lg font-medium">
            {portfolio ?? t("benchmarks.unavailable")}
          </dd>
        </div>
        <div>
          <dt className="text-xs text-muted-foreground">{t("benchmarks.return")}</dt>
          <dd className="text-lg font-medium">
            {returnValue ?? t("benchmarks.unavailable")}
          </dd>
        </div>
        <div>
          <dt className="text-xs text-muted-foreground">{t("benchmarks.excess")}</dt>
          <dd className="text-lg font-medium">
            {excess ?? t("benchmarks.unavailable")}
          </dd>
        </div>
      </dl>
    </div>
  );
}

const selectClass =
  "h-10 w-full rounded-lg border border-border bg-card px-3 text-sm font-normal text-foreground shadow-sm outline-none focus-visible:ring-2 focus-visible:ring-ring";
