import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useId, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { AppShell } from "@/components/app-shell";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { translateAccountError } from "@/features/accounts/account-form";
import {
  basisPointsToPercent,
  clampShareBps,
  formatMoney,
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
  type PortfolioDto,
} from "@/generated/tauri-bindings";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { invalidateValuation } from "@/lib/tauri/invalidate";

export function InvestmentsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [actionError, setActionError] = useState<CommandError | null>(null);
  const portfolio = useQuery({
    queryKey: ["portfolio"],
    queryFn: () => unwrapResult(commands.getPortfolio()),
  });
  const refresh = useMutation({
    mutationFn: () => unwrapResult(commands.refreshAll()),
    onSuccess: async () => {
      setActionError(null);
      await invalidateValuation(queryClient);
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const error = portfolio.error
    ? commandErrorFromUnknown(portfolio.error)
    : actionError;

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
          <Button
            disabled={refresh.isPending}
            onClick={() => refresh.mutate()}
            type="button"
          >
            {refresh.isPending ? t("investments.refreshing") : t("investments.refresh")}
          </Button>
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
        {refresh.data ? <RefreshStatus result={refresh.data.items} /> : null}
        {portfolio.data ? <PortfolioBody portfolio={portfolio.data} /> : null}
      </main>
    </AppShell>
  );
}

function RefreshStatus({
  result,
}: {
  result: Array<{
    key: string;
    ok: boolean;
    errorCode: string | null;
    message: string | null;
  }>;
}) {
  const { t } = useTranslation();
  const failed = result.filter((item) => !item.ok);
  if (result.length === 0) {
    return (
      <p className="mt-4 text-sm text-muted-foreground" role="status">
        {t("investments.nothingToRefresh")}
      </p>
    );
  }
  if (failed.length === 0) {
    return (
      <p className="mt-4 text-sm text-muted-foreground" role="status">
        {t("investments.refreshComplete")}
      </p>
    );
  }
  return (
    <div className="mt-4 text-sm text-destructive" role="status">
      <p>{t("investments.refreshPartial")}</p>
      <ul className="mt-2 list-disc pl-5">
        {failed.map((item) => (
          <li key={item.key}>
            {item.key}
            {item.message ? `: ${item.message}` : ""}
          </li>
        ))}
      </ul>
    </div>
  );
}

function PortfolioBody({ portfolio }: { portfolio: PortfolioDto }) {
  const { t } = useTranslation();
  const empty =
    portfolio.positions.length === 0 &&
    portfolio.cash.length === 0 &&
    portfolio.accounts.length === 0;
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
      <section className="rounded-2xl border border-muted bg-card px-6 py-6 shadow-sm">
        <p className="text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
          {t("investments.total")}
        </p>
        <p className="mt-2 text-4xl font-semibold tracking-tight">
          {formatMoney(portfolio.total.amount, portfolio.total.currency)}
        </p>
        <p className="mt-2 text-sm text-muted-foreground">
          {portfolio.isComplete
            ? t("investments.complete")
            : t("investments.incomplete", { coverage })}
        </p>
      </section>
      <UnvaluedList items={portfolio.unvaluedItems} />
      <PositionList portfolio={portfolio} />
      <AllocationList
        labelFor={(row) => row.name ?? row.key}
        rows={portfolio.byCurrency}
        title={t("investments.byCurrency")}
      />
      <AllocationList
        labelFor={(row) =>
          row.key === "unknown" ? t("accounts.none") : (row.name ?? row.key)
        }
        rows={portfolio.byCountry}
        title={t("investments.byCountry")}
      />
      <AllocationList
        labelFor={(row) =>
          row.key === "cash"
            ? t("accounts.cash")
            : t(`instruments.types.${row.key}`, { defaultValue: row.key })
        }
        rows={portfolio.byInstrumentType}
        title={t("investments.byType")}
      />
      <FxPanel pairs={portfolio.requiredFx} />
    </div>
  );
}

function PositionList({ portfolio }: { portfolio: PortfolioDto }) {
  const { t } = useTranslation();
  if (portfolio.positions.length === 0 && portfolio.cash.length === 0) {
    return null;
  }
  return (
    <section>
      <h2 className="text-lg font-medium">{t("investments.positions")}</h2>
      <ul className="mt-4 space-y-3">
        {portfolio.positions.map((position) => (
          <li
            className="rounded-xl border border-muted bg-card px-4 py-3"
            key={position.holdingId}
          >
            <div className="flex flex-wrap items-baseline justify-between gap-2">
              <span className="font-medium">{position.instrumentName}</span>
              <span className="text-sm text-muted-foreground">
                {position.base
                  ? formatMoney(position.base.amount, position.base.currency)
                  : t("quotes.unavailable")}
              </span>
            </div>
            <p className="mt-1 text-sm text-muted-foreground">
              {position.quantity}
              {position.native
                ? ` · ${formatMoney(position.native.amount, position.native.currency)}`
                : ""}
              {` · ${freshnessLabel(t, position.freshness)}`}
            </p>
          </li>
        ))}
        {portfolio.cash.map((cash) => (
          <li
            className="rounded-xl border border-muted bg-card px-4 py-3"
            key={cash.currency}
          >
            <div className="flex flex-wrap items-baseline justify-between gap-2">
              <span className="font-medium">{t("accounts.cash")}</span>
              <span className="text-sm text-muted-foreground">
                {formatMoney(cash.amount, cash.currency)}
              </span>
            </div>
          </li>
        ))}
      </ul>
    </section>
  );
}

function AllocationList({
  labelFor,
  rows,
  title,
}: {
  labelFor: (row: AllocationRowDto) => string;
  rows: AllocationRowDto[];
  title: string;
}) {
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
                  {formatMoney(row.amount.amount, row.amount.currency)}
                  <span className="ml-3">{percent}</span>
                </span>
              </div>
              <div
                aria-label={`${label} ${percent}`}
                aria-valuemax={10_000}
                aria-valuemin={0}
                aria-valuenow={share}
                className="mt-2 h-2 overflow-hidden rounded-full bg-muted"
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

function FxPanel({ pairs }: { pairs: FxPairStatusDto[] }) {
  const { t } = useTranslation();
  if (pairs.length === 0) {
    return null;
  }
  return (
    <section className="space-y-3">
      <h2 className="text-lg font-medium">{t("fx.title")}</h2>
      {pairs.map((pair) => (
        <FxPairCard key={`${pair.currencyA}:${pair.currencyB}`} pair={pair} />
      ))}
    </section>
  );
}

function FxPairCard({ pair }: { pair: FxPairStatusDto }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<FxRateFormValues>({ defaultValues: { rate: "" } });
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
    },
    onError: (error) => setServerError(commandErrorFromUnknown(error)),
  });

  return (
    <article className="space-y-3 rounded-xl border border-muted bg-card px-4 py-4">
      <div className="flex flex-wrap items-baseline justify-between gap-2">
        <h3 className="font-medium">
          {pair.currencyA}/{pair.currencyB}
        </h3>
        <p className="text-sm text-muted-foreground">
          {pair.selectedQuote
            ? `${pair.selectedQuote.rate} · ${t(`quotes.preference.${pair.selectedQuote.sourceKind}`)}${
                pair.selectedQuote.delayed ? ` · ${t("quotes.freshness.delayed")}` : ""
              }`
            : t("quotes.unavailable")}
        </p>
      </div>
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
            {t("quotes.addRate")}
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
    </article>
  );
}

export default InvestmentsPage;
