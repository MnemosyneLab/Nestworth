import { useQuery } from "@tanstack/react-query";
import { getRouteApi } from "@tanstack/react-router";
import { useEffect, useState, type ComponentPropsWithoutRef } from "react";
import { useTranslation } from "react-i18next";

import { AppShell } from "@/components/app-shell";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { ActivityDetailPanel } from "@/features/activity/activity-detail";
import {
  ActivityBadges,
  activityKindLabel,
  classificationLabel,
} from "@/features/activity/activity-display";
import { ActivityForm, TimezoneConfirmation } from "@/features/activity/activity-form";
import {
  ACTIVITY_CLASSIFICATIONS,
  USER_ACTIVITY_KINDS,
} from "@/features/activity/kinds";
import { mergeActivitySearch, type ActivitySearch } from "@/features/activity/search";
import { GhostButton } from "@/features/references/reference-page";
import {
  commands,
  type ActivityDetailDto,
  type ListActivitiesInput,
} from "@/generated/tauri-bindings";
import { referenceCatalogFromBootstrap } from "@/lib/reference-catalog";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { cn } from "@/lib/utils";

const activityRoute = getRouteApi("/activity");
const PAGE_SIZE = 50;

/**
 * Activity list pagination contract:
 * - Filter changes clear the URL cursor and replace the visible list with the
 *   first page for those filters.
 * - Load more requests the backend nextCursor, appends unique items, and writes
 *   that cursor into the URL.
 * - Refresh with filters restores the filter controls. Refresh with only a
 *   cursor shows that page because an opaque cursor cannot reconstruct earlier
 *   pages.
 */
export function ActivityPage() {
  const { t } = useTranslation();
  const search = activityRoute.useSearch();
  const navigate = activityRoute.useNavigate();
  const bootstrap = useBootstrapQuery();
  const household =
    bootstrap.data?.status === "ready" ? bootstrap.data.household : null;
  const catalog = referenceCatalogFromBootstrap(bootstrap.data);
  const [creating, setCreating] = useState(false);
  const [selectedId, setSelectedId] = useState<string | null>(null);
  const [items, setItems] = useState<ActivityDetailDto[]>([]);
  const filterKey = JSON.stringify(filtersFromSearch(search));

  const origin = useQuery({
    queryKey: ["history-origin"],
    queryFn: () => unwrapResult(commands.getHistoryOrigin()),
  });
  const accounts = useQuery({
    queryKey: ["accounts", true],
    queryFn: () => unwrapResult(commands.listAccounts({ includeArchived: true })),
  });
  const instruments = useQuery({
    queryKey: ["instruments", true],
    queryFn: () => unwrapResult(commands.listInstruments({ includeArchived: true })),
  });
  const list = useQuery({
    queryKey: ["activities", filterKey, search.cursor ?? null],
    queryFn: () => unwrapResult(commands.listActivities(toListInput(search))),
  });

  useEffect(() => {
    setItems([]);
  }, [filterKey]);

  useEffect(() => {
    if (!list.data) {
      return;
    }
    if (!search.cursor) {
      setItems(list.data.items);
      return;
    }
    setItems((previous) => mergeById(previous, list.data.items));
  }, [list.data, search.cursor]);

  const timezoneConfirmed = origin.data?.timezoneConfirmed ?? false;
  const error = list.error
    ? commandErrorFromUnknown(list.error)
    : origin.error
      ? commandErrorFromUnknown(origin.error)
      : null;
  const empty = items.length === 0 && !list.isPending && !error;
  const selected = items.find((item) => item.id === selectedId) ?? null;

  function patchSearch(patch: Partial<ActivitySearch>) {
    void navigate({
      search: (previous) =>
        mergeActivitySearch(previous, { ...patch, cursor: undefined }),
    });
  }

  return (
    <AppShell>
      <main className="mx-auto max-w-3xl px-8 py-10">
        <p className="mb-3 text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
          {t("activity.eyebrow")}
        </p>
        <div className="mb-6 flex flex-wrap items-center justify-between gap-3">
          <h1 className="text-3xl font-semibold tracking-tight">
            {t("activity.title")}
          </h1>
          <Button
            disabled={!origin.data}
            onClick={() => {
              setSelectedId(null);
              setCreating((value) => !value);
            }}
            type="button"
          >
            {creating ? t("references.cancel") : t("activity.add")}
          </Button>
        </div>
        {origin.data && !timezoneConfirmed ? (
          <div className="mb-6">
            <TimezoneConfirmation
              origin={origin.data}
              onConfirmed={async () => {
                await origin.refetch();
              }}
            />
          </div>
        ) : null}
        <ActivityFilters
          accounts={accounts.data ?? []}
          instruments={instruments.data ?? []}
          onPatch={patchSearch}
          search={search}
        />
        {creating && household && origin.data ? (
          <div className="mb-6">
            {timezoneConfirmed ? (
              <ActivityForm
                accounts={accounts.data ?? []}
                catalog={catalog}
                defaultCurrency={household.baseCurrency}
                instruments={instruments.data ?? []}
                mode="create"
                timezoneConfirmed={timezoneConfirmed}
                onCancel={() => setCreating(false)}
                onPosted={async () => {
                  setCreating(false);
                  await list.refetch();
                }}
              />
            ) : (
              <p className="text-sm text-muted-foreground" role="status">
                {t("activity.timezoneDescription")}
              </p>
            )}
          </div>
        ) : null}
        {list.isPending && items.length === 0 ? (
          <p role="status">{t("references.loading")}</p>
        ) : null}
        {error ? (
          <p className="text-sm text-destructive" role="alert">
            {formatCommandError(t, error)}
          </p>
        ) : null}
        {empty ? (
          <p className="text-muted-foreground">
            {hasFilters(search) ? t("activity.filterEmpty") : t("activity.empty")}
          </p>
        ) : null}
        {items.length > 0 ? (
          <ul className="space-y-3">
            {items.map((activity) => (
              <li key={activity.id}>
                <article className="rounded-xl border border-border bg-card px-4 py-4 shadow-sm">
                  <div className="flex flex-wrap items-start justify-between gap-2">
                    <div>
                      <h2 className="text-lg font-medium">
                        {activityKindLabel(t, activity.kind)}
                      </h2>
                      <p className="text-sm text-muted-foreground">
                        {activity.effectiveLocalDate} ·{" "}
                        {classificationLabel(t, activity.classification)}
                      </p>
                      <ActivityBadges
                        activity={activity}
                        archivedReference={activity.legs.some((leg) =>
                          (accounts.data ?? []).some(
                            (account) =>
                              account.id === leg.accountId && account.archivedAt,
                          ),
                        )}
                      />
                    </div>
                    <GhostButton
                      onClick={() => {
                        setCreating(false);
                        setSelectedId(activity.id);
                      }}
                      type="button"
                    >
                      {t("activity.open")}
                    </GhostButton>
                  </div>
                </article>
              </li>
            ))}
          </ul>
        ) : null}
        {list.data?.hasMore ? (
          <div className="mt-4">
            <Button
              disabled={list.isFetching}
              onClick={() => {
                if (!list.data.nextCursor) {
                  return;
                }
                void navigate({
                  search: (previous) =>
                    mergeActivitySearch(previous, {
                      cursor: list.data.nextCursor ?? undefined,
                    }),
                });
              }}
              type="button"
              variant="ghost"
            >
              {list.isFetching ? t("activity.loadingMore") : t("activity.loadMore")}
            </Button>
          </div>
        ) : null}
        {selectedId ? (
          <div className="mt-6">
            <ActivityDetailPanel
              accounts={accounts.data ?? []}
              activityId={selected?.id ?? selectedId}
              catalog={catalog}
              instruments={instruments.data ?? []}
              timezoneConfirmed={timezoneConfirmed}
              onClose={() => setSelectedId(null)}
            />
          </div>
        ) : null}
      </main>
    </AppShell>
  );
}

function ActivityFilters({
  accounts,
  instruments,
  onPatch,
  search,
}: {
  accounts: Array<{ id: string; name: string }>;
  instruments: Array<{ id: string; name: string }>;
  onPatch: (patch: Partial<ActivitySearch>) => void;
  search: ActivitySearch;
}) {
  const { t } = useTranslation();
  return (
    <div
      aria-label={t("activity.filters")}
      className="mb-6 grid gap-3 sm:grid-cols-2"
      role="search"
    >
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("activity.account")}
        <NativeSelect
          onChange={(event) => onPatch({ accountId: event.target.value || undefined })}
          value={search.accountId ?? ""}
        >
          <option value="">{t("activity.allAccounts")}</option>
          {accounts.map((account) => (
            <option key={account.id} value={account.id}>
              {account.name}
            </option>
          ))}
        </NativeSelect>
      </label>
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("activity.instrument")}
        <NativeSelect
          onChange={(event) =>
            onPatch({ instrumentId: event.target.value || undefined })
          }
          value={search.instrumentId ?? ""}
        >
          <option value="">{t("activity.allInstruments")}</option>
          {instruments.map((instrument) => (
            <option key={instrument.id} value={instrument.id}>
              {instrument.name}
            </option>
          ))}
        </NativeSelect>
      </label>
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("activity.kind")}
        <NativeSelect
          onChange={(event) => onPatch({ kind: event.target.value || undefined })}
          value={search.kind ?? ""}
        >
          <option value="">{t("activity.allKinds")}</option>
          {USER_ACTIVITY_KINDS.map((kind) => (
            <option key={kind} value={kind}>
              {t(`activity.kinds.${kind}`)}
            </option>
          ))}
        </NativeSelect>
      </label>
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("activity.classification")}
        <NativeSelect
          onChange={(event) =>
            onPatch({ classification: event.target.value || undefined })
          }
          value={search.classification ?? ""}
        >
          <option value="">{t("activity.allClassifications")}</option>
          {ACTIVITY_CLASSIFICATIONS.map((classification) => (
            <option key={classification} value={classification}>
              {t(`activity.classifications.${classification}`)}
            </option>
          ))}
        </NativeSelect>
      </label>
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("activity.startDate")}
        <Input
          onChange={(event) => onPatch({ start: event.target.value || undefined })}
          type="text"
          value={search.start ?? ""}
        />
      </label>
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("activity.endDate")}
        <Input
          onChange={(event) => onPatch({ end: event.target.value || undefined })}
          type="text"
          value={search.end ?? ""}
        />
      </label>
    </div>
  );
}

function NativeSelect({ className, ...props }: ComponentPropsWithoutRef<"select">) {
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

function toListInput(search: ActivitySearch): ListActivitiesInput {
  return {
    cursor: search.cursor ?? null,
    limit: PAGE_SIZE,
    startLocalDate: search.start ?? null,
    endLocalDate: search.end ?? null,
    accountId: search.accountId ?? null,
    instrumentId: search.instrumentId ?? null,
    kind: search.kind ?? null,
    classification: search.classification ?? null,
  };
}

function filtersFromSearch(search: ActivitySearch) {
  return {
    accountId: search.accountId ?? null,
    instrumentId: search.instrumentId ?? null,
    kind: search.kind ?? null,
    classification: search.classification ?? null,
    start: search.start ?? null,
    end: search.end ?? null,
  };
}

function hasFilters(search: ActivitySearch): boolean {
  return Boolean(
    search.accountId ||
    search.instrumentId ||
    search.kind ||
    search.classification ||
    search.start ||
    search.end,
  );
}

function mergeById(
  previous: ActivityDetailDto[],
  incoming: ActivityDetailDto[],
): ActivityDetailDto[] {
  const ids = new Set(previous.map((item) => item.id));
  return [...previous, ...incoming.filter((item) => !ids.has(item.id))];
}

export default ActivityPage;
