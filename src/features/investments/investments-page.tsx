import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useId, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { AppShell } from "@/components/app-shell";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  MarketDataBulkControls,
  RefreshResultSummary,
} from "@/features/market-data/market-data-controls";
import { translateAccountError } from "@/features/accounts/account-form";
import {
  GainSnippet,
  InstrumentAnalyticsLink,
} from "@/features/analytics/gain-snippet";
import {
  basisPointsToPercent,
  clampShareBps,
  fxRateSchema,
  type FxRateFormValues,
} from "@/features/accounts/schema";
import { applyZodIssues, FieldError } from "@/features/references/form-helpers";
import { UnvaluedList, freshnessLabel } from "@/features/valuation/status";
import {
  commands,
  type AllocationRowDto,
  type CommandError,
  type FxPairStatusDto,
  type FxQuoteRecordDto,
  type PortfolioDto,
  type ReferenceCatalogDto,
  type RefreshResultDto,
} from "@/generated/tauri-bindings";
import {
  formatReferenceMoney,
  referenceCatalogFromBootstrap,
  referenceCountryCodeLabel,
  referenceCurrencyCodeLabel,
} from "@/lib/reference-catalog";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { invalidateValuation } from "@/lib/tauri/invalidate";

export function InvestmentsPage() {
  const { t } = useTranslation();
  const bootstrap = useBootstrapQuery();
  const catalog = referenceCatalogFromBootstrap(bootstrap.data);
  const portfolio = useQuery({
    queryKey: ["portfolio"],
    queryFn: () => unwrapResult(commands.getPortfolio()),
  });
  const error = portfolio.error ? commandErrorFromUnknown(portfolio.error) : null;

  return (
    <AppShell>
      <main className="mx-auto max-w-3xl px-8 py-10">
        <p className="mb-3 text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
          {t("investments.eyebrow")}
        </p>
        <div className="flex flex-wrap items-start justify-between gap-3">
          <h1 className="text-4xl font-semibold tracking-tight">
            {t("investments.title")}
          </h1>
        </div>
        <p className="mt-2 text-sm text-muted-foreground">
          {t("investments.marketDataHelp")}
        </p>
        <div className="mt-6">
          <MarketDataBulkControls kind="all" disabled={portfolio.isPending} />
        </div>
        {portfolio.isPending ? (
          <p className="mt-10" role="status">
            {t("references.loading")}
          </p>
        ) : null}
        {error ? (
          <p className="mt-10 text-sm text-destructive" role="alert">
            {formatCommandError(t, error)}
          </p>
        ) : null}
        {portfolio.data ? (
          <PortfolioBody catalog={catalog} portfolio={portfolio.data} />
        ) : null}
      </main>
    </AppShell>
  );
}
function FxPairCard({
  catalog,
  pair,
}: {
  catalog: ReferenceCatalogDto;
  pair: FxPairStatusDto;
}) {
  const { t } = useTranslation();
  const baseCurrency = referenceCurrencyCodeLabel(t, catalog, pair.currencyB);
  const quoteCurrency = referenceCurrencyCodeLabel(t, catalog, pair.currencyA);
  const queryClient = useQueryClient();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<FxRateFormValues>({ defaultValues: { rate: "" } });
  const quotes = useQuery({
    queryKey: ["fx-quotes", pair.currencyB, pair.currencyA],
    queryFn: () =>
      unwrapResult(
        commands.listFxQuotes({
          baseCurrency: pair.currencyB,
          quoteCurrency: pair.currencyA,
        }),
      ),
  });
  const mutation = useMutation({
    mutationFn: async (values: FxRateFormValues) =>
      unwrapResult(
        commands.appendManualFxQuote({
          baseCurrency: pair.currencyB,
          quoteCurrency: pair.currencyA,
          rate: values.rate.trim(),
          quotedAt: null,
        }),
      ),
    onSuccess: async () => {
      form.reset({ rate: "" });
      await invalidateValuation(queryClient);
      await queryClient.invalidateQueries({
        queryKey: ["fx-quotes", pair.currencyB, pair.currencyA],
      });
    },
    onError: (error) => setServerError(commandErrorFromUnknown(error)),
  });

  return (
    <article className="space-y-3 rounded-xl border border-border bg-card px-4 py-4">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h3 className="font-medium">
          {t("fx.equation", {
            baseCurrency,
            quoteCurrency,
          })}
        </h3>
        <p className="text-sm text-muted-foreground">
          {pair.selectedQuote
            ? t("fx.selectedQuote", {
                baseCurrency,
                quoteCurrency,
                rate: pair.selectedRate ?? pair.selectedQuote.rate,
              })
            : t("quotes.unavailable")}
          {pair.selectedQuote
            ? ` · ${t(`quotes.preference.${pair.selectedQuote.sourceKind}`)}${
                pair.selectedQuote.delayed ? ` · ${t("quotes.freshness.delayed")}` : ""
              }`
            : ""}
        </p>
      </div>
      <p className="text-sm text-muted-foreground">
        {t("fx.rateHelp", {
          baseCurrency,
          quoteCurrency,
        })}
      </p>
      {serverError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, serverError)}
        </p>
      ) : null}
      <form
        className="flex flex-wrap items-end gap-2"
        noValidate
        onSubmit={form.handleSubmit((values) => {
          const parsed = fxRateSchema.safeParse(values);
          if (!parsed.success) {
            applyZodIssues(form, parsed.error.issues, ["rate"]);
            return;
          }
          setServerError(null);
          mutation.mutate(parsed.data);
        })}
      >
        <div className="space-y-1">
          <label className="text-sm" htmlFor={`${formId}-rate`}>
            {t("fx.rateLabel", {
              baseCurrency,
              quoteCurrency,
            })}
          </label>
          <Input
            id={`${formId}-rate`}
            inputMode="decimal"
            type="text"
            {...form.register("rate")}
          />
          <FieldError
            message={translateAccountError(t, form.formState.errors.rate?.message)}
          />
        </div>
        <Button disabled={mutation.isPending} type="submit">
          {mutation.isPending ? t("references.saving") : t("references.save")}
        </Button>
      </form>
      <FxQuoteHistory catalog={catalog} quotes={quotes.data ?? []} />
    </article>
  );
}

function FxQuoteHistory({
  catalog,
  quotes,
}: {
  catalog: ReferenceCatalogDto;
  quotes: FxQuoteRecordDto[];
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-2">
      <h4 className="text-sm font-medium">{t("fx.historyTitle")}</h4>
      <p className="text-sm text-muted-foreground">{t("fx.historyHelp")}</p>
      {quotes.length === 0 ? (
        <p className="text-sm text-muted-foreground">{t("fx.historyEmpty")}</p>
      ) : (
        <ul className="space-y-1 text-sm text-muted-foreground">
          {quotes.map((quote) => (
            <li key={quote.id}>
              {t("fx.historyItem", {
                quotedAt: quote.quotedAt,
                baseCurrency: referenceCurrencyCodeLabel(
                  t,
                  catalog,
                  quote.baseCurrency,
                ),
                quoteCurrency: referenceCurrencyCodeLabel(
                  t,
                  catalog,
                  quote.quoteCurrency,
                ),
                rate: quote.rate,
              })}
            </li>
          ))}
        </ul>
      )}
    </div>
  );
}

function PortfolioBody({
  catalog,
  portfolio,
}: {
  catalog: ReferenceCatalogDto;
  portfolio: PortfolioDto;
}) {
  const { t } = useTranslation();
  const empty =
    portfolio.positions.length === 0 &&
    portfolio.cash.length === 0 &&
    portfolio.accounts.length === 0 &&
    portfolio.requiredFx.length === 0;
  if (empty) {
    return (
      <div className="mt-10 space-y-4">
        <p className="text-muted-foreground">{t("investments.empty")}</p>
        <Link
          className="inline-flex h-10 items-center justify-center rounded-lg bg-primary px-4 text-sm font-medium text-primary-foreground shadow-sm"
          to="/accounts"
        >
          {t("accounts.add")}
        </Link>
      </div>
    );
  }

  const coverage = `${basisPointsToPercent(clampShareBps(portfolio.coverageBps))}%`;
  return (
    <div className="mt-10 space-y-10">
      <section className="rounded-2xl border border-border bg-card px-6 py-6 shadow-sm">
        <p className="text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
          {t("investments.total")}
        </p>
        <p className="mt-2 text-4xl font-semibold tracking-tight">
          {formatReferenceMoney(
            t,
            catalog,
            portfolio.total.amount,
            portfolio.total.currency,
          )}
        </p>
        <p className="mt-2 text-sm text-muted-foreground">
          {portfolio.isComplete
            ? t("investments.complete")
            : t("investments.incomplete", { coverage })}
        </p>
      </section>
      <UnvaluedList items={portfolio.unvaluedItems} />
      <PositionList catalog={catalog} portfolio={portfolio} />
      <AllocationList
        catalog={catalog}
        labelFor={(row) => referenceCurrencyCodeLabel(t, catalog, row.key)}
        rows={portfolio.byCurrency}
        title={t("investments.byCurrency")}
      />
      <AllocationList
        catalog={catalog}
        labelFor={(row) => referenceCountryCodeLabel(t, catalog, row.key)}
        rows={portfolio.byCountry}
        title={t("investments.byCountry")}
      />
      <AllocationList
        catalog={catalog}
        labelFor={(row) =>
          row.key === "cash"
            ? t("accounts.cash")
            : t(`instruments.types.${row.key}`, { defaultValue: row.key })
        }
        rows={portfolio.byInstrumentType}
        title={t("investments.byType")}
      />
      <FxPanel catalog={catalog} pairs={portfolio.requiredFx} />
    </div>
  );
}

function PositionList({
  catalog,
  portfolio,
}: {
  catalog: ReferenceCatalogDto;
  portfolio: PortfolioDto;
}) {
  const { t } = useTranslation();
  const holdingsGainQuery = useQuery({
    queryKey: ["holding-gains", { kind: "all" }],
    queryFn: () =>
      unwrapResult(commands.listHoldingGainSummaries({ period: { kind: "all" } })),
  });
  const gainByHolding = new Map(
    (holdingsGainQuery.data?.items ?? []).map((item) => [
      `${item.accountId}:${item.instrumentId}`,
      item.gain,
    ]),
  );
  if (portfolio.positions.length === 0 && portfolio.cash.length === 0) {
    return null;
  }
  return (
    <section>
      <h2 className="text-lg font-medium">{t("investments.positions")}</h2>
      <ul className="mt-4 space-y-3">
        {portfolio.positions.map((position) => {
          const gain = gainByHolding.get(
            `${position.accountId}:${position.instrumentId}`,
          );
          return (
            <li
              className="rounded-xl border border-border bg-card px-4 py-3"
              key={position.holdingId}
            >
              <div className="flex flex-wrap items-baseline justify-between gap-2">
                <span className="font-medium">{position.instrumentName}</span>
                <span className="text-sm text-muted-foreground">
                  {position.base
                    ? formatReferenceMoney(
                        t,
                        catalog,
                        position.base.amount,
                        position.base.currency,
                      )
                    : t("quotes.unavailable")}
                </span>
              </div>
              <p className="mt-1 text-sm text-muted-foreground">
                {position.quantity}
                {position.native
                  ? ` · ${formatReferenceMoney(t, catalog, position.native.amount, position.native.currency)}`
                  : ""}
                {` · ${freshnessLabel(t, position.freshness)}`}
              </p>
              <GainSnippet
                catalog={catalog}
                error={holdingsGainQuery.error}
                gain={gain}
                loading={holdingsGainQuery.isPending}
              />
              <InstrumentAnalyticsLink
                instrumentId={position.instrumentId}
                name={position.instrumentName}
              />
            </li>
          );
        })}
        {portfolio.cash.map((cash) => (
          <li
            className="rounded-xl border border-border bg-card px-4 py-3"
            key={cash.currency}
          >
            <div className="flex flex-wrap items-baseline justify-between gap-2">
              <span className="font-medium">{t("accounts.cash")}</span>
              <span className="text-sm text-muted-foreground">
                {formatReferenceMoney(t, catalog, cash.amount, cash.currency)}
              </span>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

function AllocationList({
  catalog,
  labelFor,
  rows,
  title,
}: {
  catalog: ReferenceCatalogDto;
  labelFor: (row: AllocationRowDto) => string;
  rows: AllocationRowDto[];
  title: string;
}) {
  const { t } = useTranslation();
  if (rows.length === 0) {
    return null;
  }
  return (
    <section>
      <h2 className="text-lg font-medium">{title}</h2>
      <ul className="mt-4 space-y-4">
        {rows.map((row) => {
          const label = labelFor(row);
          const share = clampShareBps(row.shareBps);
          const percent = `${basisPointsToPercent(share)}%`;
          return (
            <li key={row.key}>
              <div className="flex flex-wrap items-baseline justify-between gap-2 text-sm">
                <span>{label}</span>
                <span className="text-muted-foreground">
                  {formatReferenceMoney(
                    t,
                    catalog,
                    row.amount.amount,
                    row.amount.currency,
                  )}
                  <span className="ml-3">{percent}</span>
                </span>
              </div>
              <div
                aria-label={`${label} ${percent}`}
                aria-valuemax={10_000}
                aria-valuemin={0}
                aria-valuenow={share}
                className="mt-2 h-2 overflow-hidden rounded-full bg-surface-soft"
                role="meter"
              >
                <div
                  className="h-full rounded-full bg-primary"
                  style={{ width: `${share / 100}%` }}
                />
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}

function FxPanel({
  catalog,
  pairs,
}: {
  catalog: ReferenceCatalogDto;
  pairs: FxPairStatusDto[];
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [refreshResult, setRefreshResult] = useState<RefreshResultDto | null>(null);
  const [refreshError, setRefreshError] = useState<CommandError | null>(null);
  const refresh = useMutation({
    mutationFn: () => unwrapResult(commands.refreshRequiredFx()),
    onSuccess: async (result) => {
      setRefreshError(null);
      setRefreshResult(result);
      await invalidateValuation(queryClient);
    },
    onError: (error) => setRefreshError(commandErrorFromUnknown(error)),
  });
  if (pairs.length === 0) {
    return null;
  }
  return (
    <section className="space-y-3">
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 className="text-lg font-medium">{t("fx.title")}</h2>
        <Button
          disabled={refresh.isPending}
          onClick={() => refresh.mutate()}
          type="button"
          variant="ghost"
        >
          {refresh.isPending ? t("marketData.refreshing") : t("marketData.refreshFx")}
        </Button>
      </div>
      {refreshError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, refreshError)}
        </p>
      ) : null}
      {refreshResult ? <RefreshResultSummary result={refreshResult} /> : null}
      <MarketDataBulkControls kind="fx" />
      {pairs.map((pair) => (
        <FxPairCard
          catalog={catalog}
          key={`${pair.currencyA}:${pair.currencyB}`}
          pair={pair}
        />
      ))}
    </section>
  );
}

export default InvestmentsPage;
