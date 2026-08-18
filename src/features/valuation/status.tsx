import { useTranslation } from "react-i18next";

import type {
  AccountValuationDto,
  MoneyDto,
  ReferenceCatalogDto,
  UnvaluedItemDto,
} from "@/generated/tauri-bindings";
import { formatReferenceMoney } from "@/lib/reference-catalog";

export function freshnessLabel(
  t: ReturnType<typeof useTranslation>["t"],
  freshness: string,
): string {
  return t(`quotes.freshness.${freshness}`, { defaultValue: freshness });
}

export function ValuationSummary({
  native,
  base,
  catalog,
  freshness,
  complete,
}: {
  native?: MoneyDto | null;
  base?: MoneyDto | null;
  catalog: ReferenceCatalogDto;
  freshness: string;
  complete: boolean;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-1 text-sm text-muted-foreground">
      {native ? (
        <p>
          {t("accounts.nativeValue")}:{" "}
          {formatReferenceMoney(t, catalog, native.amount, native.currency)}
        </p>
      ) : null}
      {base ? (
        <p>
          {t("accounts.baseValue")}:{" "}
          {formatReferenceMoney(t, catalog, base.amount, base.currency)}
        </p>
      ) : null}
      <p>
        {t("accounts.freshness")}: {freshnessLabel(t, freshness)}
        {complete ? null : ` · ${t("quotes.incomplete")}`}
      </p>
    </div>
  );
}

export function UnvaluedList({ items }: { items: UnvaluedItemDto[] }) {
  const { t } = useTranslation();
  if (items.length === 0) {
    return null;
  }
  return (
    <section
      className="rounded-xl border border-destructive/40 bg-card px-4 py-4"
      role="status"
    >
      <h2 className="text-sm font-medium text-destructive">{t("overview.unvalued")}</h2>
      <ul className="mt-2 space-y-1 text-sm text-muted-foreground">
        {items.map((item) => (
          <li key={`${item.kind}:${item.id}`}>
            {item.name} —{" "}
            {t(`quotes.reasons.${item.reason}`, { defaultValue: item.reason })}
          </li>
        ))}
      </ul>
    </section>
  );
}

export function accountValuationView(valuation: AccountValuationDto) {
  return {
    native: valuation.native,
    base: valuation.base,
    freshness: valuation.freshness,
    complete: valuation.complete,
    unvaluedItems: valuation.unvaluedItems,
  };
}
