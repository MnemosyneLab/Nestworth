import { useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useEffect, useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { AppShell } from "@/components/app-shell";
import {
  AttributionPanel,
  CurrencyPanel,
  DeclarationsPanel,
  GainPanel,
  LotActionPanel,
  LotsPanel,
  ReturnPanel,
  ScopePeriodControls,
  StatusPanel,
  WorklistPanel,
  type LotAction,
} from "@/features/analytics/analytics-panels";
import {
  PAGE_SIZE,
  mergeByDeclarationId,
  mergeByLotRef,
} from "@/features/analytics/model";
import {
  isScopeReady,
  lotInstrumentId,
  mergeAnalyticsSearch,
  resolvedScope,
  toPeriodDto,
  toScopeDto,
  type AnalyticsSearch,
} from "@/features/analytics/search";
import {
  commands,
  type CostBasisDeclarationIpcDto,
  type HoldingLotDto,
} from "@/generated/tauri-bindings";
import { referenceCatalogFromBootstrap } from "@/lib/reference-catalog";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";

const analyticsRoute = getRouteApi("/analytics");

export function AnalyticsPage() {
  const { t } = useTranslation();
  const search = analyticsRoute.useSearch();
  const navigate = analyticsRoute.useNavigate();
  const bootstrap = useBootstrapQuery();
  const catalog = referenceCatalogFromBootstrap(bootstrap.data);
  const scopeDto = toScopeDto(search);
  const periodDto = toPeriodDto(search);
  const scopeReady = isScopeReady(search);
  const periodReady = periodDto !== null;
  const instrumentId = lotInstrumentId(search);
  const [lotItems, setLotItems] = useState<HoldingLotDto[]>([]);
  const [worklistItems, setWorklistItems] = useState<HoldingLotDto[]>([]);
  const [declarationItems, setDeclarationItems] = useState<
    CostBasisDeclarationIpcDto[]
  >([]);
  const [lotAction, setLotAction] = useState<LotAction | null>(null);
  const actionTriggerRef = useRef<HTMLButtonElement>(null);

  const accounts = useQuery({
    queryKey: ["accounts", true],
    queryFn: () => unwrapResult(commands.listAccounts({ includeArchived: true })),
  });
  const instruments = useQuery({
    queryKey: ["instruments", true],
    queryFn: () =>
      unwrapResult(commands.listInstruments({ includeArchived: true })),
  });
  const statusQuery = useQuery({
    queryKey: ["analytics-status", scopeDto],
    queryFn: () => unwrapResult(commands.getAnalyticsStatus({ scope: scopeDto })),
    enabled: scopeReady,
  });
  const performanceQuery = useQuery({
    queryKey: ["performance-summary", scopeDto, periodDto],
    queryFn: () =>
      unwrapResult(
        commands.getPerformanceSummary({
          scope: scopeDto,
          period: periodDto!,
        }),
      ),
    enabled: scopeReady && periodReady,
  });
  const gainQuery = useQuery({
    queryKey: ["gain-summary", scopeDto],
    queryFn: () => unwrapResult(commands.getGainSummary({ scope: scopeDto })),
    enabled: scopeReady,
  });
  const attributionQuery = useQuery({
    queryKey: ["net-worth-attribution", scopeDto, periodDto],
    queryFn: () =>
      unwrapResult(
        commands.getNetWorthAttribution({
          scope: scopeDto,
          period: periodDto!,
        }),
      ),
    enabled: scopeReady && periodReady,
  });
  const lotsQuery = useQuery({
    queryKey: ["holding-lots", instrumentId, search.lotCursor ?? null],
    queryFn: () =>
      unwrapResult(
        commands.listHoldingLots({
          scope: { kind: "instrument", instrumentId: instrumentId! },
          cursor: search.lotCursor ?? null,
          limit: PAGE_SIZE,
        }),
      ),
    enabled: Boolean(instrumentId),
  });
  const worklistQuery = useQuery({
    queryKey: ["unknown-basis-lots", scopeDto, search.worklistCursor ?? null],
    queryFn: () =>
      unwrapResult(
        commands.listUnknownBasisLots({
          scope: scopeDto,
          cursor: search.worklistCursor ?? null,
          limit: PAGE_SIZE,
        }),
      ),
    enabled: scopeReady,
  });
  const declarationsQuery = useQuery({
    queryKey: [
      "cost-basis-declarations",
      scopeDto,
      search.declarationCursor ?? null,
    ],
    queryFn: () =>
      unwrapResult(
        commands.listCostBasisDeclarations({
          scope: scopeDto,
          cursor: search.declarationCursor ?? null,
          limit: PAGE_SIZE,
        }),
      ),
    enabled: scopeReady,
  });

  useEffect(() => {
    setLotItems([]);
  }, [instrumentId]);

  useEffect(() => {
    setWorklistItems([]);
    setDeclarationItems([]);
  }, [search.scope, search.accountId, search.instrumentId]);

  useEffect(() => {
    if (!lotsQuery.data) {
      return;
    }
    if (!search.lotCursor) {
      setLotItems(lotsQuery.data.items);
      return;
    }
    setLotItems((previous) => mergeByLotRef(previous, lotsQuery.data.items));
  }, [lotsQuery.data, search.lotCursor]);

  useEffect(() => {
    if (!worklistQuery.data) {
      return;
    }
    if (!search.worklistCursor) {
      setWorklistItems(worklistQuery.data.items);
      return;
    }
    setWorklistItems((previous) =>
      mergeByLotRef(previous, worklistQuery.data.items),
    );
  }, [worklistQuery.data, search.worklistCursor]);

  useEffect(() => {
    if (!declarationsQuery.data) {
      return;
    }
    if (!search.declarationCursor) {
      setDeclarationItems(declarationsQuery.data.items);
      return;
    }
    setDeclarationItems((previous) =>
      mergeByDeclarationId(previous, declarationsQuery.data.items),
    );
  }, [declarationsQuery.data, search.declarationCursor]);

  function patchSearch(patch: Partial<AnalyticsSearch>) {
    void navigate({
      search: (previous) => mergeAnalyticsSearch(previous, patch),
    });
  }

  function closeLotAction() {
    setLotAction(null);
    actionTriggerRef.current?.focus();
  }

  const error = firstError([
    statusQuery.error,
    performanceQuery.error,
    gainQuery.error,
    attributionQuery.error,
    lotsQuery.error,
    worklistQuery.error,
    declarationsQuery.error,
    accounts.error,
    instruments.error,
  ]);
  const loading =
    (statusQuery.isPending && scopeReady) ||
    (performanceQuery.isPending && scopeReady && periodReady) ||
    (gainQuery.isPending && scopeReady);

  return (
    <AppShell>
      <main className="mx-auto max-w-5xl px-8 py-10">
        <p className="mb-3 text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
          {t("analytics.eyebrow")}
        </p>
        <h1 className="text-3xl font-semibold tracking-tight">
          {t("analytics.title")}
        </h1>
        <p className="mt-3 max-w-3xl text-muted-foreground">
          {t("analytics.description")}
        </p>
        <ScopePeriodControls
          accounts={accounts.data ?? []}
          instruments={instruments.data ?? []}
          onPatch={patchSearch}
          search={search}
        />
        {!scopeReady && resolvedScope(search) === "account" ? (
          <p className="mt-6" role="status">
            {t("analytics.selectAccount")}
          </p>
        ) : null}
        {!scopeReady && resolvedScope(search) === "instrument" ? (
          <p className="mt-6" role="status">
            {t("analytics.selectInstrument")}
          </p>
        ) : null}
        {scopeReady && !periodReady ? (
          <p className="mt-6" role="status">
            {t("analytics.selectCustomPeriod")}
          </p>
        ) : null}
        {loading ? (
          <p className="mt-6" role="status">
            {t("analytics.loading")}
          </p>
        ) : null}
        {error ? (
          <p className="mt-6 text-sm text-destructive" role="alert">
            {formatCommandError(t, error)}
          </p>
        ) : null}
        {statusQuery.data ? (
          <StatusPanel catalog={catalog} status={statusQuery.data} />
        ) : null}
        {performanceQuery.data ? (
          <ReturnPanel summary={performanceQuery.data} />
        ) : null}
        {gainQuery.data ? <GainPanel catalog={catalog} gain={gainQuery.data} /> : null}
        {gainQuery.data ? (
          <CurrencyPanel catalog={catalog} gain={gainQuery.data} />
        ) : null}
        {attributionQuery.data ? (
          <AttributionPanel attribution={attributionQuery.data} catalog={catalog} />
        ) : null}
        <LotsPanel
          accounts={accounts.data ?? []}
          action={lotAction}
          catalog={catalog}
          hasMore={lotsQuery.data?.hasMore ?? false}
          instrumentId={instrumentId}
          instruments={instruments.data ?? []}
          items={lotItems}
          loading={lotsQuery.isPending}
          loadingMore={lotsQuery.isFetching && Boolean(search.lotCursor)}
          onAction={(next, trigger) => {
            actionTriggerRef.current = trigger;
            setLotAction(next);
          }}
          onLoadMore={() => {
            if (!lotsQuery.data?.nextCursor) {
              return;
            }
            patchSearch({ lotCursor: lotsQuery.data.nextCursor });
          }}
          onSelectInstrument={(next) =>
            patchSearch({
              instrumentId: next,
              lotCursor: undefined,
            })
          }
        />
        <WorklistPanel
          accounts={accounts.data ?? []}
          action={lotAction}
          catalog={catalog}
          hasMore={worklistQuery.data?.hasMore ?? false}
          instruments={instruments.data ?? []}
          items={worklistItems}
          loading={worklistQuery.isPending && scopeReady}
          loadingMore={worklistQuery.isFetching && Boolean(search.worklistCursor)}
          onAction={(next, trigger) => {
            actionTriggerRef.current = trigger;
            setLotAction(next);
          }}
          onLoadMore={() => {
            if (!worklistQuery.data?.nextCursor) {
              return;
            }
            patchSearch({ worklistCursor: worklistQuery.data.nextCursor });
          }}
        />
        <LotActionPanel
          accounts={accounts.data ?? []}
          action={lotAction}
          instruments={instruments.data ?? []}
          items={[...lotItems, ...worklistItems]}
          onCancel={closeLotAction}
        />
        <DeclarationsPanel
          catalog={catalog}
          hasMore={declarationsQuery.data?.hasMore ?? false}
          instruments={instruments.data ?? []}
          items={declarationItems}
          loading={declarationsQuery.isPending && scopeReady}
          loadingMore={
            declarationsQuery.isFetching && Boolean(search.declarationCursor)
          }
          onLoadMore={() => {
            if (!declarationsQuery.data?.nextCursor) {
              return;
            }
            patchSearch({ declarationCursor: declarationsQuery.data.nextCursor });
          }}
        />
      </main>
    </AppShell>
  );
}

function firstError(errors: unknown[]) {
  const found = errors.find((error) => error != null);
  return found ? commandErrorFromUnknown(found) : null;
}

export default AnalyticsPage;
