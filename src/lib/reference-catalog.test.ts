import type { TFunction } from "i18next";
import { describe, expect, it } from "vitest";

import type { ReferenceCatalogDto } from "@/generated/tauri-bindings";
import {
  formatReferenceMoney,
  referenceCountryCodeLabel,
  referenceCountryNameLabel,
  referenceCurrencyCodeLabel,
  referenceInstitutionTypeLabel,
  referenceSelectOptionLabel,
} from "@/lib/reference-catalog";

const catalog: ReferenceCatalogDto = {
  currencies: [{ value: "CNY", group: "core" }],
  countries: [{ value: "SG", group: "asiaMiddleEast" }],
  institutionTypes: [{ value: "bank", group: "financial" }],
  groupIcons: [],
  groupColors: [],
  languages: [],
  appearances: [],
};

const t = ((key: string, options?: { defaultValue?: string }) => {
  const labels: Record<string, string> = {
    "reference.currencies.CNY": "Chinese Yuan",
    "reference.countries.SG": "Singapore",
    "reference.institutionTypes.bank": "Bank",
    "reference.legacy": "Unlisted",
    "accounts.none": "None",
  };
  return labels[key] ?? options?.defaultValue ?? key;
}) as TFunction;

describe("reference catalog display labels", () => {
  it("keeps names and codes together only in select options", () => {
    expect(referenceSelectOptionLabel(t, "currencies", "CNY")).toBe(
      "Chinese Yuan (CNY)",
    );
    expect(referenceInstitutionTypeLabel(t, catalog, "bank")).toBe("Bank");
  });

  it("uses compact currency and country labels in data displays", () => {
    expect(referenceCurrencyCodeLabel(t, catalog, "CNY")).toBe("CNY");
    expect(formatReferenceMoney(t, catalog, "1234.5", "CNY")).toBe("CNY 1,234.5");
    expect(referenceCountryNameLabel(t, catalog, "SG")).toBe("Singapore");
    expect(referenceCountryCodeLabel(t, catalog, "SG")).toBe("SG");
  });

  it("marks legacy values without inventing localized names", () => {
    expect(referenceInstitutionTypeLabel(t, catalog, "local_bank")).toBe(
      "local_bank (Unlisted)",
    );
    expect(referenceCurrencyCodeLabel(t, catalog, "ZZZ")).toBe("ZZZ (Unlisted)");
    expect(referenceCountryCodeLabel(t, catalog, "ZZ")).toBe("ZZ (Unlisted)");
  });
});
