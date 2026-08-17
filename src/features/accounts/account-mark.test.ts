import { describe, expect, it } from "vitest";

import { resolvedAccountLogoId } from "@/features/accounts/account-mark";
import { accountRecord, institutionRecord } from "@/test/app-harness";

describe("account logo resolution", () => {
  it("prefers the account logo, then the institution logo", () => {
    const institution = institutionRecord("i-1", "DBS", { logoAssetId: "inst-logo" });
    const withCustom = accountRecord("a-1", "Savings", { logoAssetId: "acct-logo" });
    const inherited = accountRecord("a-2", "Savings", { logoAssetId: null });
    const unknown = accountRecord("a-3", "Cash", {
      institutionId: "missing",
      logoAssetId: null,
    });
    expect(resolvedAccountLogoId(withCustom, [institution])).toBe("acct-logo");
    expect(resolvedAccountLogoId(inherited, [institution])).toBe("inst-logo");
    expect(resolvedAccountLogoId(unknown, [institution])).toBeNull();
  });
});
