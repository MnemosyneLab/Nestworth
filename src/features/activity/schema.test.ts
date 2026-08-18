import { describe, expect, it } from "vitest";

import {
  activityFormSchema,
  emptyActivityFormValues,
  previewFingerprint,
  toCreateActivityInput,
} from "@/features/activity/schema";
import { accountRecord } from "@/test/app-harness";

const accounts = [
  accountRecord("a-1", "DBS Savings"),
  accountRecord("a-2", "WeChat"),
  accountRecord("a-h", "Brokerage", {
    trackingMode: "holdings",
    primaryCategory: "investment",
    secondaryCategory: "brokerage_account",
  }),
];

describe("activity form mapping", () => {
  it.each([
    {
      name: "deposit",
      patch: { kind: "deposit" as const, accountId: "a-1", amount: "3000" },
      expectedKind: "deposit",
    },
    {
      name: "transfer",
      patch: {
        kind: "transfer" as const,
        sourceAccountId: "a-1",
        destinationAccountId: "a-2",
        sourceAmount: "3000",
        destinationAmount: "3000",
      },
      expectedKind: "transfer",
    },
    {
      name: "buy",
      patch: {
        kind: "buy" as const,
        holdingId: "h-1",
        quantity: "2",
        unitPrice: "100",
        grossAmount: "200",
        settlementCurrency: "USD",
      },
      expectedKind: "buy",
    },
    {
      name: "balance adjustment",
      patch: {
        kind: "balance_adjustment" as const,
        accountId: "a-1",
        amount: "110000",
      },
      expectedKind: "balance_adjustment",
    },
  ])("maps $name without legs or calculated totals", ({ patch, expectedKind }) => {
    const values = {
      ...emptyActivityFormValues("deposit", "CNY", "2026-08-18", "09:30"),
      ...patch,
    };
    const parsed = activityFormSchema.safeParse(values);
    expect(parsed.success).toBe(true);
    if (!parsed.success) {
      return;
    }
    const input = toCreateActivityInput(parsed.data, accounts);
    expect(input.kind).toBe(expectedKind);
    expect(input).not.toHaveProperty("legs");
    expect(JSON.stringify(input)).not.toMatch(/externalFlow|resulting|grossTotal/);
    if (input.kind === "buy") {
      expect(input.grossAmount).toBe("200");
      expect(input.quantity).toBe("2");
      expect(input.unitPrice).toBe("100");
    }
    if (input.kind === "transfer") {
      expect(input.sourceAmount).toBe("3000");
      expect(input.destinationAmount).toBe("3000");
    }
  });

  it("changes the preview fingerprint when transfer endpoints change", () => {
    const values = {
      ...emptyActivityFormValues("transfer", "CNY", "2026-08-18", "09:30"),
      kind: "transfer" as const,
      sourceAccountId: "a-1",
      destinationAccountId: "a-2",
      sourceAmount: "3000",
      destinationAmount: "3000",
    };
    const first = toCreateActivityInput(values, accounts);
    const second = toCreateActivityInput(
      { ...values, destinationAccountId: "a-h", destinationCurrency: "USD" },
      accounts,
    );
    expect(previewFingerprint(first)).not.toBe(previewFingerprint(second));
  });
});
