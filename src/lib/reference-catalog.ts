import type { TFunction } from "i18next";

import { formatMoney } from "@/features/accounts/schema";
import type {
  BootstrapDto,
  ReferenceCatalogDto,
  ReferenceOptionDto,
} from "@/generated/tauri-bindings";

export const EMPTY_REFERENCE_CATALOG: ReferenceCatalogDto = {
  currencies: [],
  countries: [],
  institutionTypes: [],
  groupIcons: [],
  groupColors: [],
  languages: [],
  appearances: [],
};

export function referenceCatalogFromBootstrap(
  bootstrap: BootstrapDto | undefined,
): ReferenceCatalogDto {
  return bootstrap?.status === "ready"
    ? bootstrap.referenceCatalog
    : EMPTY_REFERENCE_CATALOG;
}

export function hasReferenceValue(
  values: readonly string[] | readonly ReferenceOptionDto[],
  value: string,
): boolean {
  return values.some(
    (item) => (typeof item === "string" ? item : item.value) === value,
  );
}

export function withLegacyOption(
  options: readonly ReferenceOptionDto[],
  value: string,
): ReferenceOptionDto[] {
  if (!value || hasReferenceValue(options, value)) {
    return [...options];
  }
  return [{ value, group: "legacy" }, ...options];
}

export function referenceSelectOptionLabel(
  t: TFunction,
  namespace: string,
  value: string,
): string {
  const label = t(`reference.${namespace}.${value}`, { defaultValue: value });
  return `${label} (${value})`;
}

export function legacyOptionLabel(t: TFunction, value: string): string {
  return `${value} (${t("reference.legacy")})`;
}

export function referenceInstitutionTypeLabel(
  t: TFunction,
  catalog: ReferenceCatalogDto,
  value: string,
): string {
  return hasReferenceValue(catalog.institutionTypes, value)
    ? t(`reference.institutionTypes.${value}`, { defaultValue: value })
    : legacyOptionLabel(t, value);
}

export function referenceCurrencyCodeLabel(
  t: TFunction,
  catalog: ReferenceCatalogDto,
  value: string,
): string {
  return hasReferenceValue(catalog.currencies, value)
    ? value
    : legacyOptionLabel(t, value);
}

export function referenceCountryNameLabel(
  t: TFunction,
  catalog: ReferenceCatalogDto,
  value: string,
): string {
  if (value === "unknown") {
    return t("accounts.none");
  }
  return hasReferenceValue(catalog.countries, value)
    ? t(`reference.countries.${value}`, { defaultValue: value })
    : legacyOptionLabel(t, value);
}

export function referenceCountryCodeLabel(
  t: TFunction,
  catalog: ReferenceCatalogDto,
  value: string,
): string {
  if (value === "unknown") {
    return t("accounts.none");
  }
  return hasReferenceValue(catalog.countries, value)
    ? value
    : legacyOptionLabel(t, value);
}

export function formatReferenceMoney(
  t: TFunction,
  catalog: ReferenceCatalogDto,
  amount: string,
  currency: string,
): string {
  return formatMoney(amount, referenceCurrencyCodeLabel(t, catalog, currency));
}

export function referenceGroupLabel(t: TFunction, namespace: string, group: string) {
  return t(`reference.${namespace}.${group}`, { defaultValue: group });
}

export function groupReferenceOptions(
  options: readonly ReferenceOptionDto[],
): Array<[string, ReferenceOptionDto[]]> {
  const groups = new Map<string, ReferenceOptionDto[]>();
  for (const option of options) {
    const values = groups.get(option.group) ?? [];
    values.push(option);
    groups.set(option.group, values);
  }
  return [...groups.entries()];
}
