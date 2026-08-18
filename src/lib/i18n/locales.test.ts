import { describe, expect, it } from "vitest";

import en from "@/locales/en/common.json";
import zhCN from "@/locales/zh-CN/common.json";

describe("locale resources", () => {
  it("keeps English and Simplified Chinese keys synchronized", () => {
    expect(flattenKeys(zhCN)).toEqual(flattenKeys(en));
  });
});

function flattenKeys(value: unknown, prefix = ""): string[] {
  if (value === null || typeof value !== "object" || Array.isArray(value)) {
    return prefix ? [prefix] : [];
  }
  return Object.entries(value as Record<string, unknown>).flatMap(([key, nested]) =>
    flattenKeys(nested, prefix ? `${prefix}.${key}` : key),
  );
}
