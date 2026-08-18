import { useQuery } from "@tanstack/react-query";
import { useEffect, useState } from "react";
import { useTranslation } from "react-i18next";

import {
  ActivityBadges,
  activityKindLabel,
  classificationLabel,
  formatMagnitude,
} from "@/features/activity/activity-display";
import { Button } from "@/components/ui/button";
import {
  commands,
  type AccountRecordDto,
  type AccountTimelineItemDto,
  type ReferenceCatalogDto,
} from "@/generated/tauri-bindings";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";

export function AccountTimeline({
  account,
  catalog,
}: {
  account: AccountRecordDto;
  catalog: ReferenceCatalogDto;
}) {
  const { t } = useTranslation();
  const [items, setItems] = useState<AccountTimelineItemDto[]>([]);
  const [cursor, setCursor] = useState<string | null>(null);
  const timeline = useQuery({
    queryKey: ["account-timeline", account.id, cursor],
    queryFn: () =>
      unwrapResult(
        commands.getAccountTimeline({
          accountId: account.id,
          cursor,
          limit: 50,
        }),
      ),
  });

  useEffect(() => {
    setItems([]);
    setCursor(null);
  }, [account.id]);

  useEffect(() => {
    if (!timeline.data) {
      return;
    }
    if (!cursor) {
      setItems(timeline.data.items);
      return;
    }
    setItems((previous) => [...previous, ...timeline.data.items]);
  }, [timeline.data, cursor]);

  const error = timeline.error ? commandErrorFromUnknown(timeline.error) : null;

  return (
    <section className="space-y-3 rounded-xl border border-border bg-card px-4 py-4 shadow-sm">
      <h2 className="text-lg font-medium">{t("activity.timeline.title")}</h2>
      {timeline.isPending && items.length === 0 ? (
        <p role="status">{t("references.loading")}</p>
      ) : null}
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, error)}
        </p>
      ) : null}
      {items.length === 0 && !timeline.isPending && !error ? (
        <p className="text-sm text-muted-foreground">{t("activity.timeline.empty")}</p>
      ) : null}
      <ol className="space-y-3">
        {items.map((item, index) => (
          <li key={timelineItemKey(item, index)}>
            <TimelineItem catalog={catalog} item={item} />
          </li>
        ))}
      </ol>
      {timeline.data?.hasMore ? (
        <Button
          disabled={timeline.isFetching}
          onClick={() => setCursor(timeline.data.nextCursor)}
          type="button"
          variant="ghost"
        >
          {timeline.isFetching ? t("activity.loadingMore") : t("activity.loadMore")}
        </Button>
      ) : null}
    </section>
  );
}

function TimelineItem({
  catalog,
  item,
}: {
  catalog: ReferenceCatalogDto;
  item: AccountTimelineItemDto;
}) {
  const { t } = useTranslation();
  if (item.kind === "origin") {
    return (
      <article>
        <p className="text-xs uppercase tracking-wide text-muted-foreground">
          {t("activity.badges.legacyOrigin")}
        </p>
        <p className="font-medium">{t("activity.timeline.origin")}</p>
        <p className="text-sm text-muted-foreground">
          {item.localDate} · {t("activity.timeline.openingState")}
        </p>
      </article>
    );
  }
  if (item.kind === "observation") {
    return (
      <article>
        <p className="text-xs uppercase tracking-wide text-muted-foreground">
          {t("activity.badges.observation")}
        </p>
        <p className="font-medium">
          {t(`activity.components.${item.componentKind}`, {
            defaultValue: item.componentKind,
          })}
        </p>
        <p className="text-sm text-muted-foreground">
          {formatMagnitude(t, catalog, item.amount, item.currency)}
        </p>
      </article>
    );
  }
  if (item.kind === "account_state") {
    return (
      <article>
        <p className="text-xs uppercase tracking-wide text-muted-foreground">
          {t("activity.timeline.accountState")}
        </p>
        <p className="font-medium">
          {item.archived
            ? t("activity.timeline.archived")
            : t("activity.timeline.restored")}
        </p>
        <p className="text-sm text-muted-foreground">
          {t(`accounts.primaries.${item.primaryCategory}`, {
            defaultValue: item.primaryCategory,
          })}
        </p>
      </article>
    );
  }
  return (
    <article>
      <p className="font-medium">{activityKindLabel(t, item.activity.kind)}</p>
      <p className="text-sm text-muted-foreground">
        {item.activity.effectiveLocalDate} ·{" "}
        {classificationLabel(t, item.activity.classification)}
      </p>
      <ActivityBadges activity={item.activity} />
    </article>
  );
}

function timelineItemKey(item: AccountTimelineItemDto, index: number): string {
  if (item.kind === "activity") {
    return `activity:${item.activity.id}`;
  }
  return `${item.kind}:${item.id}:${index}`;
}
