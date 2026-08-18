import { describe, expect, it } from "vitest";

import {
  accountMatchesSearch,
  SHARED_OWNER,
  validateAccountSearch,
} from "@/features/accounts/search";
import { accountRecord } from "@/test/app-harness";

describe("account search filters", () => {
  const dbs = accountRecord("a-1", "DBS Savings");
  const wechat = accountRecord("a-2", "WeChat", {
    institutionId: null,
    owners: [{ memberId: "m-2", memberName: "Spouse", shareBps: 10_000 }],
  });
  const home = accountRecord("a-3", "Home", {
    primaryCategory: "property",
    secondaryCategory: "real_estate",
    institutionId: null,
    groupId: "g-1",
    owners: [
      { memberId: "m-1", memberName: "Walt", shareBps: 5_000 },
      { memberId: "m-2", memberName: "Spouse", shareBps: 5_000 },
    ],
  });

  it("parses only non-empty string params", () => {
    expect(
      validateAccountSearch({
        owner: "m-1",
        category: "investment",
        institution: "",
        extra: 1,
      }),
    ).toEqual({
      owner: "m-1",
      category: "investment",
    });
  });

  it("partitions sole owners and shared accounts", () => {
    expect(accountMatchesSearch(dbs, { owner: "m-1" })).toBe(true);
    expect(accountMatchesSearch(home, { owner: "m-1" })).toBe(false);
    expect(accountMatchesSearch(wechat, { owner: "m-1" })).toBe(false);
    expect(accountMatchesSearch(home, { owner: SHARED_OWNER })).toBe(true);
    expect(accountMatchesSearch(dbs, { owner: SHARED_OWNER })).toBe(false);
  });

  it("intersects owner with category, institution, and group", () => {
    expect(accountMatchesSearch(home, { category: "property" })).toBe(true);
    expect(accountMatchesSearch(dbs, { category: "property" })).toBe(false);
    expect(accountMatchesSearch(dbs, { institution: "i-1" })).toBe(true);
    expect(accountMatchesSearch(home, { institution: "i-1" })).toBe(false);
    expect(accountMatchesSearch(home, { owner: SHARED_OWNER, group: "g-1" })).toBe(
      true,
    );
    expect(accountMatchesSearch(home, { owner: SHARED_OWNER, group: "g-2" })).toBe(
      false,
    );
  });
});
