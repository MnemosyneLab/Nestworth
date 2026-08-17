import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useTranslation } from "react-i18next";

import { AppShell } from "@/components/app-shell";
import { basisPointsToPercent, formatMoney } from "@/features/accounts/schema";
import type {
  BreakdownRowDto,
  HouseholdDto,
  OverviewDto,
} from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";

export function OverviewPage({ household }: { household: HouseholdDto }) {
  const { t } = useTranslation();
  const overview = useQuery({
    queryKey: ["overview"],
    queryFn: () => unwrapResult(commands.getOverview()),
  });
  const error = overview.error ? commandErrorFromUnknown(overview.error) : null;

  return (
    <AppShell>
      <main className="mx-auto max-w-3xl px-8 py-10">
        <p className="mb-3 text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
          {t("overview.eyebrow")}
        </p>
        <h1 className="text-4xl font-semibold tracking-tight">{household.name}</h1>
        <p className="mt-3 text-lg text-muted-foreground">
          {t("overview.baseCurrency", { currency: household.baseCurrency })}
        </p>
        {overview.isPending ? (
          <p className="mt-10" role="status">
            {t("references.loading")}
          </p>
        ) : null}
        {error ? (
          <p className="mt-10 text-sm text-destructive" role="alert">
            {formatCommandError(t, error)}
          </p>
        ) : null}
        {overview.data ? <OverviewBody overview={overview.data} /> : null}
      </main>
    </AppShell>
  );
}

function OverviewBody({ overview }: { overview: OverviewDto }) {
  const { t } = useTranslation();
  if (overview.accountCount === 0) {
    return (
      <div className="mt-10 space-y-4">
        <p className="text-muted-foreground">{t("overview.empty")}</p>
        <p className="text-muted-foreground">{t("overview.emptyHint")}</p>
        <Link
          className="inline-flex h-10 items-center justify-center rounded-lg bg-primary px-4 text-sm font-medium text-primary-foreground shadow-sm"
          to="/accounts"
        >
          {t("accounts.add")}
        </Link>
      </div>
    );
  }

  return (
    <div className="mt-10 space-y-10">
      <section className="rounded-2xl border border-muted bg-card px-6 py-6 shadow-sm">
        <p className="text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
          {t("overview.netWorth")}
        </p>
        <p className="mt-2 text-4xl font-semibold tracking-tight">
          {formatMoney(overview.netWorth.amount, overview.netWorth.currency)}
        </p>
        <dl className="mt-6 grid gap-4 sm:grid-cols-2">
          <div>
            <dt className="text-sm text-muted-foreground">{t("overview.assets")}</dt>
            <dd className="mt-1 text-xl font-medium">
              {formatMoney(overview.assets.amount, overview.assets.currency)}
            </dd>
          </div>
          <div>
            <dt className="text-sm text-muted-foreground">
              {t("overview.liabilities")}
            </dt>
            <dd className="mt-1 text-xl font-medium">
              {formatMoney(overview.liabilities.amount, overview.liabilities.currency)}
            </dd>
          </div>
        </dl>
      </section>
      <BreakdownList
        rows={overview.byCategory}
        title={t("overview.byCategory")}
        labelFor={(row) => t(`accounts.primaries.${row.key}`)}
      />
      <BreakdownList
        rows={overview.byMember}
        title={t("overview.byMember")}
        labelFor={(row) => row.name ?? t("accounts.none")}
      />
      <BreakdownList
        rows={overview.byInstitution}
        title={t("overview.byInstitution")}
        labelFor={(row) => row.name ?? t("accounts.none")}
      />
      <BreakdownList
        rows={overview.byGroup}
        title={t("overview.byGroup")}
        labelFor={(row) => row.name ?? t("accounts.none")}
      />
    </div>
  );
}

function BreakdownList({
  labelFor,
  rows,
  title,
}: {
  labelFor: (row: BreakdownRowDto) => string;
  rows: BreakdownRowDto[];
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
          const percent = `${basisPointsToPercent(row.shareBps)}%`;
          return (
            <li key={`${row.key}:${row.id ?? "none"}`}>
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
                aria-valuenow={row.shareBps}
                className="mt-2 h-2 overflow-hidden rounded-full bg-muted"
                role="meter"
              >
                <div
                  className="h-full rounded-full bg-primary"
                  style={{ width: `${row.shareBps / 100}%` }}
                />
              </div>
            </li>
          );
        })}
      </ul>
    </section>
  );
}
