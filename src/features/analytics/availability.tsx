import { useTranslation } from "react-i18next";

import type {
  DateAvailabilityDto,
  DateRangeAvailabilityDto,
  MoneyAvailabilityDto,
  ReferenceCatalogDto,
  SignedMoneyAvailabilityDto,
} from "@/generated/tauri-bindings";
import { formatReferenceMoney } from "@/lib/reference-catalog";

export function availabilityReason(
  t: ReturnType<typeof useTranslation>["t"],
  reason: string,
): string {
  return t(`analytics.reasons.${reason}`, {
    defaultValue: t("analytics.reasons.unavailableFallback"),
  });
}

export function UnavailableNotice({
  blockingDates,
  reason,
  unconvertibleFlowCount,
}: {
  blockingDates: string[];
  reason: string;
  unconvertibleFlowCount?: number;
}) {
  const { t } = useTranslation();
  const dates =
    blockingDates.length > 0
      ? t("analytics.unavailable.blockingDates", {
          dates: blockingDates.join(", "),
        })
      : t("analytics.unavailable.blockingDatesNone");
  return (
    <div className="space-y-1 text-sm" role="status">
      <p>
        {t("analytics.unavailable.reason")}: {availabilityReason(t, reason)}
      </p>
      <p>{dates}</p>
      {unconvertibleFlowCount === undefined ? null : (
        <p>
          {t("analytics.unavailable.unconvertibleFlows", {
            count: unconvertibleFlowCount,
          })}
        </p>
      )}
      <p className="text-muted-foreground">
        {t("analytics.availability.neverZeroStandIn")}
      </p>
    </div>
  );
}

export function formatSignedMoney(
  t: ReturnType<typeof useTranslation>["t"],
  catalog: ReferenceCatalogDto,
  money: { amount: string; currency: string },
): string {
  return formatReferenceMoney(t, catalog, money.amount, money.currency);
}

export function signedMoneyLabel(
  t: ReturnType<typeof useTranslation>["t"],
  catalog: ReferenceCatalogDto,
  value: SignedMoneyAvailabilityDto,
): string {
  if (value.kind === "unavailable") {
    return availabilityReason(t, value.reason);
  }
  return formatSignedMoney(t, catalog, value.value);
}

export function moneyLabel(
  t: ReturnType<typeof useTranslation>["t"],
  catalog: ReferenceCatalogDto,
  value: MoneyAvailabilityDto,
): string {
  if (value.kind === "unavailable") {
    return availabilityReason(t, value.reason);
  }
  return formatSignedMoney(t, catalog, value.value);
}

export function dateLabel(
  t: ReturnType<typeof useTranslation>["t"],
  value: DateAvailabilityDto,
): string {
  if (value.kind === "unavailable") {
    return availabilityReason(t, value.reason);
  }
  return value.value;
}

export function dateRangeLabel(
  t: ReturnType<typeof useTranslation>["t"],
  value: DateRangeAvailabilityDto,
): string {
  if (value.kind === "unavailable") {
    return availabilityReason(t, value.reason);
  }
  return t("analytics.status.usableHistoryRange", {
    start: value.startLocalDate,
    end: value.endLocalDate,
  });
}

export function methodLabel(
  t: ReturnType<typeof useTranslation>["t"],
  method: string,
): string {
  if (method === "twr" || method === "xirr" || method === "fifo") {
    return t(`analytics.methods.${method}`);
  }
  return method;
}

export function flowAssumptionLabel(
  t: ReturnType<typeof useTranslation>["t"],
  assumption: string,
): string {
  if (assumption === "startOfDay") {
    return t("analytics.methods.startOfDay");
  }
  return assumption;
}

export function feeKindLabel(
  t: ReturnType<typeof useTranslation>["t"],
  kind: string,
): string {
  if (kind === "tradeCommission") {
    return t("analytics.gain.tradeCommission");
  }
  return t(`activity.feeKinds.${kind}`, { defaultValue: kind });
}

export function incomeKindLabel(
  t: ReturnType<typeof useTranslation>["t"],
  kind: string,
): string {
  return t(`activity.incomeKinds.${kind}`, { defaultValue: kind });
}

export function completeLabel(
  t: ReturnType<typeof useTranslation>["t"],
  complete: boolean,
): string {
  return complete
    ? t("analytics.gain.completeYes")
    : t("analytics.gain.completeNo");
}
