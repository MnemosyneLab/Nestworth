import {
  type ComponentPropsWithoutRef,
  type KeyboardEvent,
} from "react";
import { useTranslation } from "react-i18next";

import { formatMoneyAmount } from "@/features/accounts/schema";
import {
  UnavailableNotice,
  completeLabel,
  dateLabel,
  dateRangeLabel,
  feeKindLabel,
  flowAssumptionLabel,
  formatSignedMoney,
  incomeKindLabel,
  methodLabel,
  moneyLabel,
  signedMoneyLabel,
} from "@/features/analytics/availability";
import { AnalyticsBarChart } from "@/features/analytics/chart";
import {
  DeclarationForm,
  RevocationForm,
} from "@/features/analytics/declaration-form";
import { attributionComponents, lotKey } from "@/features/analytics/model";
import {
  ANALYTICS_PERIODS,
  ANALYTICS_SCOPES,
  resolvedPeriod,
  resolvedScope,
  type AnalyticsSearch,
} from "@/features/analytics/search";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { GhostButton } from "@/features/references/reference-page";
import type {
  AccountRecordDto,
  AnalyticsStatusDto,
  CostBasisDeclarationIpcDto,
  GainSummaryIpcDto,
  HoldingLotDto,
  InstrumentRecordDto,
  MoneyAvailabilityDto,
  NetWorthAttributionIpcDto,
  PerformanceSummaryDto,
  ReferenceCatalogDto,
  SignedMoneyAvailabilityDto,
} from "@/generated/tauri-bindings";
import { cn } from "@/lib/utils";

export type LotAction = { kind: "declare" | "revoke"; lot: HoldingLotDto };

export function ScopePeriodControls({
  accounts,
  instruments,
  onPatch,
  search,
}: {
  accounts: AccountRecordDto[];
  instruments: InstrumentRecordDto[];
  onPatch: (patch: Partial<AnalyticsSearch>) => void;
  search: AnalyticsSearch;
}) {
  const { t } = useTranslation();
  const scope = resolvedScope(search);
  const period = resolvedPeriod(search);

  function onScopeKey(event: KeyboardEvent<HTMLDivElement>) {
    const index = ANALYTICS_SCOPES.indexOf(scope);
    const next = moveIndex(event, index, ANALYTICS_SCOPES.length);
    if (next === null) {
      return;
    }
    event.preventDefault();
    onPatch({
      scope: ANALYTICS_SCOPES[next],
      lotCursor: undefined,
      worklistCursor: undefined,
      declarationCursor: undefined,
    });
  }

  function onPeriodKey(event: KeyboardEvent<HTMLDivElement>) {
    const index = ANALYTICS_PERIODS.indexOf(period);
    const next = moveIndex(event, index, ANALYTICS_PERIODS.length);
    if (next === null) {
      return;
    }
    event.preventDefault();
    onPatch({ period: ANALYTICS_PERIODS[next] });
  }

  return (
    <div className="mt-8 space-y-4">
      <div
        aria-label={t("analytics.scope.label")}
        className="flex flex-wrap gap-1"
        onKeyDown={onScopeKey}
        role="radiogroup"
      >
        {ANALYTICS_SCOPES.map((value) => {
          const selected = value === scope;
          return (
            <Button
              aria-checked={selected}
              className="h-9 px-3"
              key={value}
              onClick={() =>
                onPatch({
                  scope: value,
                  lotCursor: undefined,
                  worklistCursor: undefined,
                  declarationCursor: undefined,
                })
              }
              role="radio"
              tabIndex={selected ? 0 : -1}
              type="button"
              variant={selected ? "primary" : "ghost"}
            >
              {t(`analytics.scope.${value}`)}
            </Button>
          );
        })}
      </div>
      {scope === "account" ? (
        <label className="grid max-w-sm gap-1 text-sm text-muted-foreground">
          {t("analytics.scope.accountSelect")}
          <NativeSelect
            onChange={(event) =>
              onPatch({
                accountId: event.target.value || undefined,
                lotCursor: undefined,
                worklistCursor: undefined,
                declarationCursor: undefined,
              })
            }
            value={search.accountId ?? ""}
          >
            <option value="">{t("analytics.selectAccount")}</option>
            {accounts.map((account) => (
              <option key={account.id} value={account.id}>
                {account.name}
              </option>
            ))}
          </NativeSelect>
        </label>
      ) : null}
      {scope === "instrument" ? (
        <label className="grid max-w-sm gap-1 text-sm text-muted-foreground">
          {t("analytics.scope.instrumentSelect")}
          <NativeSelect
            onChange={(event) =>
              onPatch({
                instrumentId: event.target.value || undefined,
                lotCursor: undefined,
                worklistCursor: undefined,
                declarationCursor: undefined,
              })
            }
            value={search.instrumentId ?? ""}
          >
            <option value="">{t("analytics.selectInstrument")}</option>
            {instruments.map((instrument) => (
              <option key={instrument.id} value={instrument.id}>
                {instrument.name}
              </option>
            ))}
          </NativeSelect>
        </label>
      ) : null}
      <div
        aria-label={t("analytics.period.label")}
        className="flex flex-wrap gap-1"
        onKeyDown={onPeriodKey}
        role="radiogroup"
      >
        {ANALYTICS_PERIODS.map((value) => {
          const selected = value === period;
          return (
            <Button
              aria-checked={selected}
              className="h-9 px-3"
              key={value}
              onClick={() => onPatch({ period: value })}
              role="radio"
              tabIndex={selected ? 0 : -1}
              type="button"
              variant={selected ? "primary" : "ghost"}
            >
              {t(`analytics.period.${value}`)}
            </Button>
          );
        })}
      </div>
      {period === "custom" ? (
        <div className="grid max-w-xl gap-3 sm:grid-cols-2">
          <label className="grid gap-1 text-sm text-muted-foreground">
            {t("analytics.period.start")}
            <Input
              onChange={(event) =>
                onPatch({ start: event.target.value || undefined })
              }
              value={search.start ?? ""}
            />
          </label>
          <label className="grid gap-1 text-sm text-muted-foreground">
            {t("analytics.period.end")}
            <Input
              onChange={(event) =>
                onPatch({ end: event.target.value || undefined })
              }
              value={search.end ?? ""}
            />
          </label>
        </div>
      ) : null}
    </div>
  );
}

export function StatusPanel({
  catalog,
  status,
}: {
  catalog: ReferenceCatalogDto;
  status: AnalyticsStatusDto;
}) {
  const { t } = useTranslation();
  return (
    <section className="mt-10 space-y-3">
      <h2 className="text-lg font-medium">{t("analytics.status.title")}</h2>
      <dl className="grid gap-3 text-sm sm:grid-cols-2">
        <div>
          <dt className="text-muted-foreground">
            {t("analytics.status.usableHistory")}
          </dt>
          <dd>
            {status.usableHistory.kind === "unavailable" ? (
              <UnavailableNotice
                blockingDates={status.usableHistory.blockingDates}
                reason={status.usableHistory.reason}
              />
            ) : (
              dateRangeLabel(t, status.usableHistory)
            )}
          </dd>
        </div>
        <div>
          <dt className="text-muted-foreground">
            {t("analytics.status.earliestSnapshot")}
          </dt>
          <dd>
            {status.earliestCompleteSnapshotOn.kind === "unavailable" ? (
              <UnavailableNotice
                blockingDates={status.earliestCompleteSnapshotOn.blockingDates}
                reason={status.earliestCompleteSnapshotOn.reason}
              />
            ) : (
              dateLabel(t, status.earliestCompleteSnapshotOn)
            )}
          </dd>
        </div>
        <div>
          <dt className="text-muted-foreground">
            {t("analytics.status.blockingDates")}
          </dt>
          <dd>
            {status.blockingDates.length > 0
              ? status.blockingDates.join(", ")
              : t("analytics.status.none")}
          </dd>
        </div>
        <div>
          <dt className="text-muted-foreground">
            {t("analytics.status.unknownBasisCount")}
          </dt>
          <dd>{status.unknownBasisLotCount}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">
            {t("analytics.status.unknownBasisValue")}
          </dt>
          <dd>
            {status.unknownBasisValue.kind === "unavailable" ? (
              <UnavailableNotice
                blockingDates={status.unknownBasisValue.blockingDates}
                reason={status.unknownBasisValue.reason}
              />
            ) : (
              moneyLabel(t, catalog, status.unknownBasisValue)
            )}
          </dd>
        </div>
      </dl>
      {status.unknownBasisLotCount > 0 ? (
        <p className="text-sm" role="status">
          {t("analytics.availability.unknownBasisExcluded")}
        </p>
      ) : null}
    </section>
  );
}

export function ReturnPanel({ summary }: { summary: PerformanceSummaryDto }) {
  const { t } = useTranslation();
  const twr = summary.twr;
  const xirr = summary.xirr;
  return (
    <section className="mt-10 space-y-4">
      <h2 className="text-lg font-medium">{t("analytics.return.title")}</h2>
      <p className="text-sm text-muted-foreground">
        {t("analytics.methods.startOfDayDescription")}
      </p>
      <p className="text-sm text-muted-foreground">
        {t("analytics.return.fractionHelp")}
      </p>
      {twr.kind === "unavailable" ? (
        <UnavailableNotice
          blockingDates={twr.blockingDates}
          reason={twr.reason}
        />
      ) : null}
      {xirr.kind === "unavailable" ? (
        <UnavailableNotice
          blockingDates={xirr.blockingDates}
          reason={xirr.reason}
        />
      ) : null}
      <div className="overflow-x-auto">
        <table
          aria-label={t("analytics.return.table")}
          className="w-full min-w-[36rem] border-collapse text-left text-sm"
        >
          <thead>
            <tr>
              <HeaderCell>{t("analytics.return.method")}</HeaderCell>
              <HeaderCell>{t("analytics.return.flowAssumption")}</HeaderCell>
              <HeaderCell>{t("analytics.return.cumulative")}</HeaderCell>
              <HeaderCell>{t("analytics.return.annualized")}</HeaderCell>
              <HeaderCell>{t("analytics.return.skippedDays")}</HeaderCell>
              <HeaderCell>{t("analytics.return.linkedDays")}</HeaderCell>
            </tr>
          </thead>
          <tbody>
            <tr className="border-b border-border" tabIndex={0}>
              <td className="py-2 pr-3">
                {twr.kind === "available"
                  ? methodLabel(t, twr.method)
                  : t("analytics.return.twrTitle")}
              </td>
              <td className="py-2 pr-3">
                {twr.kind === "available"
                  ? flowAssumptionLabel(t, twr.flowAssumption)
                  : t("analytics.availability.unavailable")}
              </td>
              <td className="py-2 pr-3">
                {twr.kind === "available"
                  ? twr.cumulative
                  : t("analytics.availability.unavailable")}
              </td>
              <td className="py-2 pr-3">
                {twr.kind === "available"
                  ? (twr.annualized ??
                    t("analytics.availability.withheldAnnualization"))
                  : t("analytics.availability.unavailable")}
              </td>
              <td className="py-2 pr-3">
                {twr.kind === "available" ? twr.skippedDays : "—"}
              </td>
              <td className="py-2">
                {twr.kind === "available" ? twr.linkedDays : "—"}
              </td>
            </tr>
            <tr className="border-b border-border" tabIndex={0}>
              <td className="py-2 pr-3">
                {xirr.kind === "available"
                  ? methodLabel(t, xirr.method)
                  : t("analytics.return.xirrTitle")}
              </td>
              <td className="py-2 pr-3">—</td>
              <td className="py-2 pr-3">
                {xirr.kind === "available"
                  ? xirr.cumulative
                  : t("analytics.availability.unavailable")}
              </td>
              <td className="py-2 pr-3">
                {xirr.kind === "available"
                  ? (xirr.annualized ??
                    t("analytics.availability.withheldAnnualization"))
                  : t("analytics.availability.unavailable")}
              </td>
              <td className="py-2 pr-3">—</td>
              <td className="py-2">—</td>
            </tr>
          </tbody>
        </table>
      </div>
    </section>
  );
}

export function GainPanel({
  catalog,
  gain,
}: {
  catalog: ReferenceCatalogDto;
  gain: GainSummaryIpcDto;
}) {
  const { t } = useTranslation();
  const rows = gainRows(t, catalog, gain);
  const chartAmounts = availableAmounts([
    gain.realizedGross,
    gain.realizedNet,
    gain.allocatedFees,
    gain.unrealizedGross,
    gain.unexplainedDisposal,
  ]);
  return (
    <section className="mt-10 space-y-4">
      <h2 className="text-lg font-medium">{t("analytics.gain.title")}</h2>
      <p className="text-sm">{t("analytics.lots.policy")}</p>
      {!gain.basisComplete ? (
        <p role="status">{t("analytics.gain.unknownBasisExclusion")}</p>
      ) : null}
      {!gain.inputComplete || !gain.decompositionComplete ? (
        <p role="status">{t("analytics.incomplete")}</p>
      ) : null}
      {chartAmounts.length > 0 ? (
        <AnalyticsBarChart
          amounts={chartAmounts}
          label={t("analytics.gain.chart")}
        />
      ) : null}
      <MetricTable ariaLabel={t("analytics.gain.table")} rows={rows} />
      <BucketTable
        ariaLabel={t("analytics.gain.incomeByKind")}
        catalog={catalog}
        empty={gain.income.length === 0}
        items={gain.income.map((item) => ({
          kind: incomeKindLabel(t, item.incomeKind),
          attributed: item.attributedInstrumentId,
          amount: item.amount,
        }))}
        title={t("analytics.gain.incomeByKind")}
      />
      <BucketTable
        ariaLabel={t("analytics.gain.feesByKind")}
        catalog={catalog}
        empty={gain.fees.length === 0}
        items={gain.fees.map((item) => ({
          kind: feeKindLabel(t, item.feeKind),
          attributed: item.attributedInstrumentId,
          amount: item.amount,
        }))}
        title={t("analytics.gain.feesByKind")}
      />
    </section>
  );
}

export function CurrencyPanel({
  catalog,
  gain,
}: {
  catalog: ReferenceCatalogDto;
  gain: GainSummaryIpcDto;
}) {
  const { t } = useTranslation();
  const chartAmounts = availableAmounts([
    gain.instrumentMovement,
    gain.currencyMovement,
  ]);
  return (
    <section className="mt-10 space-y-4">
      <h2 className="text-lg font-medium">{t("analytics.currency.title")}</h2>
      {!gain.decompositionComplete ? (
        <p role="status">{t("analytics.currency.incomplete")}</p>
      ) : null}
      {gain.instrumentMovement.kind === "unavailable" ? (
        <UnavailableNotice
          blockingDates={gain.instrumentMovement.blockingDates}
          reason={gain.instrumentMovement.reason}
        />
      ) : null}
      {gain.currencyMovement.kind === "unavailable" ? (
        <UnavailableNotice
          blockingDates={gain.currencyMovement.blockingDates}
          reason={gain.currencyMovement.reason}
        />
      ) : null}
      {chartAmounts.length > 0 ? (
        <AnalyticsBarChart
          amounts={chartAmounts}
          label={t("analytics.currency.chart")}
        />
      ) : null}
      <MetricTable
        ariaLabel={t("analytics.currency.table")}
        rows={[
          {
            label: t("analytics.currency.instrumentMovement"),
            value: signedMoneyCell(t, catalog, gain.instrumentMovement),
          },
          {
            label: t("analytics.currency.currencyMovement"),
            value: signedMoneyCell(t, catalog, gain.currencyMovement),
          },
        ]}
      />
    </section>
  );
}

export function AttributionPanel({
  attribution,
  catalog,
}: {
  attribution: NetWorthAttributionIpcDto;
  catalog: ReferenceCatalogDto;
}) {
  const { t } = useTranslation();
  if (attribution.kind === "unavailable") {
    return (
      <section className="mt-10 space-y-4">
        <h2 className="text-lg font-medium">{t("analytics.attribution.title")}</h2>
        <UnavailableNotice
          blockingDates={attribution.blockingDates}
          reason={attribution.reason}
          unconvertibleFlowCount={attribution.unconvertibleFlowCount}
        />
      </section>
    );
  }
  const value = attribution.value;
  const components = attributionComponents(value);
  return (
    <section className="mt-10 space-y-4">
      <h2 className="text-lg font-medium">{t("analytics.attribution.title")}</h2>
      <p className="text-sm text-muted-foreground">
        {t("analytics.attribution.methodNote")}
      </p>
      <p className="text-sm">{t("analytics.attribution.unexplainedAlwaysShown")}</p>
      {!value.basisComplete ? (
        <p role="status">{t("analytics.gain.unknownBasisExclusion")}</p>
      ) : null}
      <AnalyticsBarChart
        amounts={components.map((item) => item.money.amount)}
        label={t("analytics.attribution.chart")}
      />
      <div className="overflow-x-auto">
        <table
          aria-label={t("analytics.attribution.table")}
          className="w-full min-w-[36rem] border-collapse text-left text-sm"
        >
          <thead>
            <tr>
              <HeaderCell>{t("analytics.attribution.component")}</HeaderCell>
              <HeaderCell>{t("analytics.attribution.amount")}</HeaderCell>
            </tr>
          </thead>
          <tbody>
            <tr className="border-b border-border" tabIndex={0}>
              <td className="py-2 pr-3">{t("analytics.attribution.startOn")}</td>
              <td className="py-2">{value.startOn}</td>
            </tr>
            <tr className="border-b border-border" tabIndex={0}>
              <td className="py-2 pr-3">{t("analytics.attribution.endOn")}</td>
              <td className="py-2">{value.endOn}</td>
            </tr>
            <tr className="border-b border-border" tabIndex={0}>
              <td className="py-2 pr-3">
                {t("analytics.attribution.startNetWorth")}
              </td>
              <td className="py-2">
                {formatSignedMoney(t, catalog, value.startNetWorth)}
              </td>
            </tr>
            <tr className="border-b border-border" tabIndex={0}>
              <td className="py-2 pr-3">{t("analytics.attribution.endNetWorth")}</td>
              <td className="py-2">
                {formatSignedMoney(t, catalog, value.endNetWorth)}
              </td>
            </tr>
            <tr className="border-b border-border" tabIndex={0}>
              <td className="py-2 pr-3">{t("analytics.attribution.delta")}</td>
              <td className="py-2">{formatSignedMoney(t, catalog, value.delta)}</td>
            </tr>
            {components.map((item) => (
              <tr className="border-b border-border" key={item.key} tabIndex={0}>
                <td className="py-2 pr-3">
                  {t(`analytics.attribution.components.${item.key}`)}
                </td>
                <td className="py-2">
                  {formatSignedMoney(t, catalog, item.money)}
                </td>
              </tr>
            ))}
          </tbody>
        </table>
      </div>
    </section>
  );
}

export function LotsPanel({
  accounts,
  action,
  catalog,
  hasMore,
  instrumentId,
  instruments,
  items,
  loading,
  loadingMore,
  onAction,
  onLoadMore,
  onSelectInstrument,
}: {
  accounts: AccountRecordDto[];
  action: LotAction | null;
  catalog: ReferenceCatalogDto;
  hasMore: boolean;
  instrumentId: string | undefined;
  instruments: InstrumentRecordDto[];
  items: HoldingLotDto[];
  loading: boolean;
  loadingMore: boolean;
  onAction: (action: LotAction, trigger: HTMLButtonElement) => void;
  onLoadMore: () => void;
  onSelectInstrument: (instrumentId: string | undefined) => void;
}) {
  const { t } = useTranslation();
  return (
    <section className="mt-10 space-y-4">
      <h2 className="text-lg font-medium">{t("analytics.lots.title")}</h2>
      <p className="text-sm text-muted-foreground">{t("analytics.lots.policy")}</p>
      <label className="grid max-w-sm gap-1 text-sm text-muted-foreground">
        {t("analytics.lots.instrument")}
        <NativeSelect
          onChange={(event) => onSelectInstrument(event.target.value || undefined)}
          value={instrumentId ?? ""}
        >
          <option value="">{t("analytics.lots.selectInstrument")}</option>
          {instruments.map((instrument) => (
            <option key={instrument.id} value={instrument.id}>
              {instrument.name}
            </option>
          ))}
        </NativeSelect>
      </label>
      {!instrumentId ? (
        <p role="status">{t("analytics.lots.selectInstrument")}</p>
      ) : null}
      {instrumentId && loading && items.length === 0 ? (
        <p role="status">{t("analytics.loading")}</p>
      ) : null}
      {instrumentId && !loading && items.length === 0 ? (
        <p>{t("analytics.lots.empty")}</p>
      ) : null}
      {items.length > 0 ? (
        <LotTable
          accounts={accounts}
          action={action}
          catalog={catalog}
          instruments={instruments}
          items={items}
          label={t("analytics.lots.table")}
          onAction={onAction}
        />
      ) : null}
      {hasMore ? (
        <Button disabled={loadingMore} onClick={onLoadMore} type="button" variant="ghost">
          {loadingMore ? t("analytics.lots.loadingMore") : t("analytics.lots.loadMore")}
        </Button>
      ) : null}
    </section>
  );
}

export function WorklistPanel({
  accounts,
  action,
  catalog,
  hasMore,
  instruments,
  items,
  loading,
  loadingMore,
  onAction,
  onLoadMore,
}: {
  accounts: AccountRecordDto[];
  action: LotAction | null;
  catalog: ReferenceCatalogDto;
  hasMore: boolean;
  instruments: InstrumentRecordDto[];
  items: HoldingLotDto[];
  loading: boolean;
  loadingMore: boolean;
  onAction: (action: LotAction, trigger: HTMLButtonElement) => void;
  onLoadMore: () => void;
}) {
  const { t } = useTranslation();
  return (
    <section className="mt-10 space-y-4">
      <h2 className="text-lg font-medium">{t("analytics.worklist.title")}</h2>
      <p className="text-sm">{t("analytics.worklist.explanation")}</p>
      {loading && items.length === 0 ? (
        <p role="status">{t("analytics.loading")}</p>
      ) : null}
      {!loading && items.length === 0 ? (
        <p>{t("analytics.worklist.empty")}</p>
      ) : null}
      {items.length > 0 ? (
        <LotTable
          accounts={accounts}
          action={action}
          catalog={catalog}
          instruments={instruments}
          items={items}
          label={t("analytics.worklist.table")}
          onAction={onAction}
        />
      ) : null}
      {hasMore ? (
        <Button disabled={loadingMore} onClick={onLoadMore} type="button" variant="ghost">
          {loadingMore
            ? t("analytics.worklist.loadingMore")
            : t("analytics.worklist.loadMore")}
        </Button>
      ) : null}
    </section>
  );
}

export function DeclarationsPanel({
  catalog,
  hasMore,
  instruments,
  items,
  loading,
  loadingMore,
  onLoadMore,
}: {
  catalog: ReferenceCatalogDto;
  hasMore: boolean;
  instruments: InstrumentRecordDto[];
  items: CostBasisDeclarationIpcDto[];
  loading: boolean;
  loadingMore: boolean;
  onLoadMore: () => void;
}) {
  const { t } = useTranslation();
  return (
    <section className="mt-10 space-y-4">
      <h2 className="text-lg font-medium">{t("analytics.declarations.title")}</h2>
      {loading && items.length === 0 ? (
        <p role="status">{t("analytics.loading")}</p>
      ) : null}
      {!loading && items.length === 0 ? (
        <p>{t("analytics.declarations.empty")}</p>
      ) : null}
      {items.length > 0 ? (
        <div className="overflow-x-auto">
          <table
            aria-label={t("analytics.declarations.table")}
            className="w-full min-w-[40rem] border-collapse text-left text-sm"
          >
            <thead>
              <tr>
                <HeaderCell>{t("analytics.lots.instrument")}</HeaderCell>
                <HeaderCell>{t("analytics.declarations.cost")}</HeaderCell>
                <HeaderCell>{t("analytics.declarations.acquiredOn")}</HeaderCell>
                <HeaderCell>{t("analytics.declarations.createdAt")}</HeaderCell>
                <HeaderCell>{t("analytics.lots.basis")}</HeaderCell>
              </tr>
            </thead>
            <tbody>
              {items.map((item) => {
                const instrument = instruments.find(
                  (row) => row.id === item.instrumentId,
                );
                const cost =
                  item.declaredCost && item.declaredCurrency
                    ? formatSignedMoney(t, catalog, {
                        amount: item.declaredCost,
                        currency: item.declaredCurrency,
                      })
                    : t("analytics.status.none");
                return (
                  <tr className="border-b border-border" key={item.id} tabIndex={0}>
                    <td className="py-2 pr-3">
                      {instrument?.name ?? item.instrumentId}
                    </td>
                    <td className="py-2 pr-3">{cost}</td>
                    <td className="py-2 pr-3">
                      {item.acquiredOn ?? t("analytics.status.none")}
                    </td>
                    <td className="py-2 pr-3">{item.createdAt}</td>
                    <td className="py-2">
                      {item.isRevocation
                        ? t("analytics.declarations.revocation")
                        : t("analytics.declarations.supply")}
                    </td>
                  </tr>
                );
              })}
            </tbody>
          </table>
        </div>
      ) : null}
      {hasMore ? (
        <Button disabled={loadingMore} onClick={onLoadMore} type="button" variant="ghost">
          {loadingMore
            ? t("analytics.declarations.loadingMore")
            : t("analytics.declarations.loadMore")}
        </Button>
      ) : null}
    </section>
  );
}

export function LotActionPanel({
  accounts,
  action,
  instruments,
  items,
  onCancel,
}: {
  accounts: AccountRecordDto[];
  action: LotAction | null;
  instruments: InstrumentRecordDto[];
  items: HoldingLotDto[];
  onCancel: () => void;
}) {
  if (!action || !items.some((item) => lotKey(item) === lotKey(action.lot))) {
    return null;
  }
  const lot = action.lot;
  const instrument = instruments.find((item) => item.id === lot.instrumentId);
  const account = accounts.find((item) => item.id === lot.accountId);
  if (action.kind === "declare") {
    return (
      <DeclarationForm
        accountName={account?.name ?? lot.accountId}
        instrumentName={instrument?.name ?? lot.instrumentId}
        lot={lot}
        onCancel={onCancel}
        quoteCurrency={instrument?.quoteCurrency ?? ""}
      />
    );
  }
  return (
    <RevocationForm
      accountName={account?.name ?? lot.accountId}
      instrumentName={instrument?.name ?? lot.instrumentId}
      lot={lot}
      onCancel={onCancel}
      quoteCurrency={instrument?.quoteCurrency ?? ""}
    />
  );
}

function LotTable({
  accounts,
  action,
  catalog,
  instruments,
  items,
  label,
  onAction,
}: {
  accounts: AccountRecordDto[];
  action: LotAction | null;
  catalog: ReferenceCatalogDto;
  instruments: InstrumentRecordDto[];
  items: HoldingLotDto[];
  label: string;
  onAction: (action: LotAction, trigger: HTMLButtonElement) => void;
}) {
  const { t } = useTranslation();
  return (
    <div className="overflow-x-auto">
      <table
        aria-label={label}
        className="w-full min-w-[56rem] border-collapse text-left text-sm"
      >
        <thead>
          <tr>
            <HeaderCell>{t("analytics.lots.instrument")}</HeaderCell>
            <HeaderCell>{t("analytics.lots.account")}</HeaderCell>
            <HeaderCell>{t("analytics.lots.acquiredAt")}</HeaderCell>
            <HeaderCell>{t("analytics.lots.quantity")}</HeaderCell>
            <HeaderCell>{t("analytics.lots.cost")}</HeaderCell>
            <HeaderCell>{t("analytics.lots.currentValue")}</HeaderCell>
            <HeaderCell>{t("analytics.lots.unrealized")}</HeaderCell>
            <HeaderCell>{t("analytics.lots.basis")}</HeaderCell>
            <HeaderCell>{t("analytics.lots.source")}</HeaderCell>
            <th className="border-b border-border py-2 font-medium" scope="col" />
          </tr>
        </thead>
        <tbody>
          {items.map((lot) => {
            const instrument = instruments.find(
              (item) => item.id === lot.instrumentId,
            );
            const account = accounts.find((item) => item.id === lot.accountId);
            return (
              <tr className="border-b border-border align-top" key={lotKey(lot)}>
                <td className="py-2 pr-3">{instrument?.name ?? lot.instrumentId}</td>
                <td className="py-2 pr-3">{account?.name ?? lot.accountId}</td>
                <td className="py-2 pr-3">{lot.acquiredAt}</td>
                <td className="py-2 pr-3">
                  {formatMoneyAmount(lot.quantityRemaining)}
                </td>
                <td className="py-2 pr-3">{moneyCell(t, catalog, lot.cost)}</td>
                <td className="py-2 pr-3">
                  {moneyCell(t, catalog, lot.currentValue)}
                </td>
                <td className="py-2 pr-3">
                  {signedMoneyCell(t, catalog, lot.unrealizedGross)}
                </td>
                <td className="py-2 pr-3">
                  {lot.basis === "unknown"
                    ? t("analytics.basis.unknown")
                    : lot.isDeclared
                      ? t("analytics.basis.declared")
                      : t("analytics.basis.known")}
                </td>
                <td className="py-2 pr-3">
                  {t(`analytics.sourceKind.${lot.lotRef.sourceKind}`, {
                    defaultValue: lot.lotRef.sourceKind,
                  })}
                </td>
                <td className="py-2">
                  {lot.basis === "unknown" ? (
                    <GhostButton
                      aria-pressed={
                        action?.kind === "declare" &&
                        lotKey(action.lot) === lotKey(lot)
                      }
                      onClick={(event) =>
                        onAction({ kind: "declare", lot }, event.currentTarget)
                      }
                      type="button"
                    >
                      {t("analytics.declare.action")}
                    </GhostButton>
                  ) : lot.isDeclared ? (
                    <GhostButton
                      aria-pressed={
                        action?.kind === "revoke" &&
                        lotKey(action.lot) === lotKey(lot)
                      }
                      onClick={(event) =>
                        onAction({ kind: "revoke", lot }, event.currentTarget)
                      }
                      type="button"
                    >
                      {t("analytics.revoke.action")}
                    </GhostButton>
                  ) : null}
                </td>
              </tr>
            );
          })}
        </tbody>
      </table>
    </div>
  );
}

function MetricTable({
  ariaLabel,
  rows,
}: {
  ariaLabel: string;
  rows: Array<{ label: string; value: string }>;
}) {
  const { t } = useTranslation();
  return (
    <div className="overflow-x-auto">
      <table
        aria-label={ariaLabel}
        className="w-full min-w-[24rem] border-collapse text-left text-sm"
      >
        <thead>
          <tr>
            <HeaderCell>{t("analytics.attribution.component")}</HeaderCell>
            <HeaderCell>{t("analytics.attribution.amount")}</HeaderCell>
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr className="border-b border-border" key={row.label} tabIndex={0}>
              <td className="py-2 pr-3">{row.label}</td>
              <td className="py-2">{row.value}</td>
            </tr>
          ))}
        </tbody>
      </table>
    </div>
  );
}

function BucketTable({
  ariaLabel,
  catalog,
  empty,
  items,
  title,
}: {
  ariaLabel: string;
  catalog: ReferenceCatalogDto;
  empty: boolean;
  items: Array<{
    kind: string;
    attributed: string | null;
    amount: { amount: string; currency: string };
  }>;
  title: string;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-2">
      <h3 className="font-medium">{title}</h3>
      {empty ? (
        <p className="text-sm text-muted-foreground">{t("analytics.empty")}</p>
      ) : (
        <div className="overflow-x-auto">
          <table
            aria-label={ariaLabel}
            className="w-full min-w-[28rem] border-collapse text-left text-sm"
          >
            <thead>
              <tr>
                <HeaderCell>{t("analytics.gain.kind")}</HeaderCell>
                <HeaderCell>{t("analytics.gain.instrumentAttribution")}</HeaderCell>
                <HeaderCell>{t("analytics.attribution.amount")}</HeaderCell>
              </tr>
            </thead>
            <tbody>
              {items.map((item, index) => (
                <tr
                  className="border-b border-border"
                  key={`${item.kind}:${item.attributed ?? "none"}:${index}`}
                  tabIndex={0}
                >
                  <td className="py-2 pr-3">{item.kind}</td>
                  <td className="py-2 pr-3">
                    {item.attributed ?? t("analytics.gain.unattributed")}
                  </td>
                  <td className="py-2">
                    {formatSignedMoney(t, catalog, item.amount)}
                  </td>
                </tr>
              ))}
            </tbody>
          </table>
        </div>
      )}
    </div>
  );
}

function HeaderCell({ children }: { children: string }) {
  return (
    <th className="border-b border-border py-2 pr-3 font-medium" scope="col">
      {children}
    </th>
  );
}

function NativeSelect({
  className,
  ...props
}: ComponentPropsWithoutRef<"select">) {
  return (
    <select
      {...props}
      className={cn(
        "h-10 w-full rounded-lg border border-border bg-card px-3 text-sm text-foreground shadow-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
    />
  );
}

function gainRows(
  t: ReturnType<typeof useTranslation>["t"],
  catalog: ReferenceCatalogDto,
  gain: GainSummaryIpcDto,
): Array<{ label: string; value: string }> {
  return [
    {
      label: t("analytics.gain.realizedGross"),
      value: signedMoneyCell(t, catalog, gain.realizedGross),
    },
    {
      label: t("analytics.gain.realizedNet"),
      value: signedMoneyCell(t, catalog, gain.realizedNet),
    },
    {
      label: t("analytics.gain.allocatedFees"),
      value: signedMoneyCell(t, catalog, gain.allocatedFees),
    },
    {
      label: t("analytics.gain.unrealizedGross"),
      value: signedMoneyCell(t, catalog, gain.unrealizedGross),
    },
    {
      label: t("analytics.gain.unexplainedDisposal"),
      value: signedMoneyCell(t, catalog, gain.unexplainedDisposal),
    },
    {
      label: t("analytics.gain.unknownBasisQuantity"),
      value: gain.unknownBasisQuantity,
    },
    {
      label: t("analytics.gain.unknownBasisValue"),
      value: moneyCell(t, catalog, gain.unknownBasisValue),
    },
    {
      label: t("analytics.gain.basisComplete"),
      value: completeLabel(t, gain.basisComplete),
    },
    {
      label: t("analytics.gain.inputComplete"),
      value: completeLabel(t, gain.inputComplete),
    },
    {
      label: t("analytics.gain.decompositionComplete"),
      value: completeLabel(t, gain.decompositionComplete),
    },
  ];
}

function signedMoneyCell(
  t: ReturnType<typeof useTranslation>["t"],
  catalog: ReferenceCatalogDto,
  value: SignedMoneyAvailabilityDto,
): string {
  if (value.kind === "unavailable") {
    return signedMoneyLabel(t, catalog, value);
  }
  return formatSignedMoney(t, catalog, value.value);
}

function moneyCell(
  t: ReturnType<typeof useTranslation>["t"],
  catalog: ReferenceCatalogDto,
  value: MoneyAvailabilityDto,
): string {
  if (value.kind === "unavailable") {
    return moneyLabel(t, catalog, value);
  }
  return formatSignedMoney(t, catalog, value.value);
}

function availableAmounts(values: SignedMoneyAvailabilityDto[]): string[] {
  return values
    .filter((value): value is Extract<SignedMoneyAvailabilityDto, { kind: "available" }> =>
      value.kind === "available",
    )
    .map((value) => value.value.amount);
}

function moveIndex(
  event: KeyboardEvent<HTMLDivElement>,
  index: number,
  length: number,
): number | null {
  if (event.key === "ArrowRight" || event.key === "ArrowDown") {
    return (index + 1) % length;
  }
  if (event.key === "ArrowLeft" || event.key === "ArrowUp") {
    return (index - 1 + length) % length;
  }
  return null;
}
