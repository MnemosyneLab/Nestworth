import type { TFunction } from "i18next";
import { useTranslation } from "react-i18next";

import type {
  ActivityDetailDto,
  ActivityLegDto,
  ActivityPreviewDto,
  ReferenceCatalogDto,
  ResultingEndpointDto,
} from "@/generated/tauri-bindings";
import { formatReferenceMoney } from "@/lib/reference-catalog";

export function activityKindLabel(t: TFunction, kind: string): string {
  return t(`activity.kinds.${kind}`, { defaultValue: kind });
}

export function classificationLabel(t: TFunction, classification: string): string {
  return t(`activity.classifications.${classification}`, {
    defaultValue: classification,
  });
}

export function ActivityBadges({
  activity,
  archivedReference,
}: {
  activity: ActivityDetailDto;
  archivedReference?: boolean;
}) {
  const { t } = useTranslation();
  const badges: string[] = [];
  if (activity.reversed) {
    badges.push(t("activity.badges.reversed"));
  }
  if (activity.isReversal) {
    badges.push(t("activity.badges.reversal"));
  }
  if (activity.isReplacement || activity.corrects) {
    badges.push(t("activity.badges.corrected"));
  }
  if (archivedReference) {
    badges.push(t("activity.badges.archivedReference"));
  }
  if (badges.length === 0) {
    return null;
  }
  return (
    <ul className="mt-1 flex flex-wrap gap-1">
      {badges.map((badge) => (
        <li
          className="rounded-full bg-surface-soft px-2 py-0.5 text-xs uppercase tracking-wide text-muted-foreground"
          key={badge}
        >
          {badge}
        </li>
      ))}
    </ul>
  );
}

export function ActivityPreviewPanel({
  catalog,
  preview,
}: {
  catalog: ReferenceCatalogDto;
  preview: ActivityPreviewDto;
}) {
  const { t } = useTranslation();
  return (
    <section
      aria-label={t("activity.previewTitle")}
      className="space-y-3 rounded-xl border border-border bg-surface-soft px-4 py-4"
    >
      <h3 className="text-sm font-medium">{t("activity.previewTitle")}</h3>
      <p className="text-sm text-muted-foreground">{t("activity.previewHelp")}</p>
      <p className="text-sm">
        {activityKindLabel(t, preview.activity.kind)} ·{" "}
        {classificationLabel(t, preview.activity.classification)}
      </p>
      <ActivityLegs catalog={catalog} legs={preview.activity.legs} />
      {preview.resulting.length > 0 ? (
        <div>
          <h4 className="text-sm font-medium">{t("activity.resulting")}</h4>
          <ul className="mt-2 space-y-1 text-sm">
            {preview.resulting.map((endpoint) => (
              <li key={resultingKey(endpoint)}>
                {endpoint.accountName} ·{" "}
                {t(`activity.components.${endpoint.componentKind}`, {
                  defaultValue: endpoint.componentKind,
                })}
                :{" "}
                {t("activity.before", {
                  value: formatMagnitude(
                    t,
                    catalog,
                    endpoint.before,
                    endpoint.currency,
                  ),
                })}
                {"; "}
                {t("activity.after", {
                  value: formatMagnitude(t, catalog, endpoint.after, endpoint.currency),
                })}
              </li>
            ))}
          </ul>
        </div>
      ) : null}
    </section>
  );
}

export function ActivityLegs({
  catalog,
  legs,
}: {
  catalog: ReferenceCatalogDto;
  legs: ActivityLegDto[];
}) {
  const { t } = useTranslation();
  if (legs.length === 0) {
    return null;
  }
  return (
    <div>
      <h4 className="text-sm font-medium">{t("activity.legs")}</h4>
      <ul className="mt-2 space-y-2 text-sm">
        {legs.map((leg) => (
          <li key={leg.id}>
            <span className="font-medium">{leg.accountName}</span>
            {leg.instrumentName ? ` · ${leg.instrumentName}` : ""}
            {` · ${t(`activity.roles.${leg.role}`, { defaultValue: leg.role })}`}
            {` · ${t(`activity.directions.${leg.direction}`, { defaultValue: leg.direction })}`}
            {leg.amount && leg.currency
              ? ` · ${formatReferenceMoney(t, catalog, leg.amount, leg.currency)}`
              : ""}
            {leg.quantity ? ` · ${leg.quantity}` : ""}
            {leg.fxRate ? ` · ${leg.fxRate}` : ""}
            {` · ${classificationLabel(t, leg.classification)}`}
          </li>
        ))}
      </ul>
    </div>
  );
}

export function formatMagnitude(
  t: TFunction,
  catalog: ReferenceCatalogDto,
  value: string,
  currency: string | null,
): string {
  if (currency) {
    return formatReferenceMoney(t, catalog, value, currency);
  }
  return value;
}

function resultingKey(endpoint: ResultingEndpointDto): string {
  return [
    endpoint.accountId,
    endpoint.componentKind,
    endpoint.holdingId ?? "",
    endpoint.currency ?? "",
  ].join(":");
}
