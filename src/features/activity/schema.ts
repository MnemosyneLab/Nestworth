import { z } from "zod";

import {
  AMOUNT_PATTERN,
  CURRENCY_PATTERN,
  DATE_PATTERN,
  FX_RATE_PATTERN,
  QUANTITY_PATTERN,
  UNIT_PRICE_PATTERN,
} from "@/features/accounts/schema";
import {
  FEE_KINDS,
  INCOME_KINDS,
  USER_ACTIVITY_KINDS,
  isUserActivityKind,
  monetaryComponentForTrackingMode,
  type UserActivityKind,
} from "@/features/activity/kinds";
import type { AccountRecordDto, CreateActivityInput } from "@/generated/tauri-bindings";
import { emptyToNull } from "@/lib/empty-to-null";

export const TIME_PATTERN = /^([01]\d|2[0-3]):[0-5]\d$/;

export type ActivityFormValues = {
  kind: UserActivityKind;
  transferType: "cash" | "position";
  localDate: string;
  localTime: string;
  ambiguousOffset: string;
  note: string;
  accountId: string;
  component: string;
  amount: string;
  currency: string;
  holdingId: string;
  instrumentId: string;
  quantity: string;
  sourceAccountId: string;
  destinationAccountId: string;
  sourceAmount: string;
  sourceCurrency: string;
  destinationAmount: string;
  destinationCurrency: string;
  fxRate: string;
  sourceHoldingId: string;
  destinationHoldingId: string;
  unitPrice: string;
  grossAmount: string;
  settlementCurrency: string;
  feeAmount: string;
  confirmZeroUnitPrice: boolean;
  incomeKind: string;
  feeKind: string;
  liabilityAccountId: string;
  principalAmount: string;
  principalCurrency: string;
  cashAccountId: string;
  cashAmount: string;
  cashCurrency: string;
};

export const ACTIVITY_FORM_FIELDS: Array<keyof ActivityFormValues> = [
  "kind",
  "transferType",
  "localDate",
  "localTime",
  "ambiguousOffset",
  "note",
  "accountId",
  "component",
  "amount",
  "currency",
  "holdingId",
  "instrumentId",
  "quantity",
  "sourceAccountId",
  "destinationAccountId",
  "sourceAmount",
  "sourceCurrency",
  "destinationAmount",
  "destinationCurrency",
  "fxRate",
  "sourceHoldingId",
  "destinationHoldingId",
  "unitPrice",
  "grossAmount",
  "settlementCurrency",
  "feeAmount",
  "confirmZeroUnitPrice",
  "incomeKind",
  "feeKind",
  "liabilityAccountId",
  "principalAmount",
  "principalCurrency",
  "cashAccountId",
  "cashAmount",
  "cashCurrency",
];

export function emptyActivityFormValues(
  kind: UserActivityKind,
  currency: string,
  localDate: string,
  localTime: string,
): ActivityFormValues {
  return {
    kind,
    transferType: "cash",
    localDate,
    localTime,
    ambiguousOffset: "",
    note: "",
    accountId: "",
    component: "account_value",
    amount: "",
    currency,
    holdingId: "",
    instrumentId: "",
    quantity: "",
    sourceAccountId: "",
    destinationAccountId: "",
    sourceAmount: "",
    sourceCurrency: currency,
    destinationAmount: "",
    destinationCurrency: currency,
    fxRate: "",
    sourceHoldingId: "",
    destinationHoldingId: "",
    unitPrice: "",
    grossAmount: "",
    settlementCurrency: currency,
    feeAmount: "",
    confirmZeroUnitPrice: false,
    incomeKind: "salary",
    feeKind: "bank_fee",
    liabilityAccountId: "",
    principalAmount: "",
    principalCurrency: currency,
    cashAccountId: "",
    cashAmount: "",
    cashCurrency: currency,
  };
}

const requiredText = z.string().trim().min(1, "required");
const amountField = requiredText.regex(AMOUNT_PATTERN, "amount");
const quantityField = requiredText.regex(QUANTITY_PATTERN, "quantity");
const currencyField = requiredText.regex(CURRENCY_PATTERN, "currency");
const optionalAmount = z
  .string()
  .trim()
  .refine((value) => value.length === 0 || AMOUNT_PATTERN.test(value), "amount");
const optionalFxRate = z
  .string()
  .trim()
  .refine((value) => value.length === 0 || FX_RATE_PATTERN.test(value), "rate");

export const activityFormSchema = z
  .object({
    kind: z.enum(USER_ACTIVITY_KINDS),
    transferType: z.enum(["cash", "position"]),
    localDate: requiredText.regex(DATE_PATTERN, "date"),
    localTime: requiredText.regex(TIME_PATTERN, "time"),
    ambiguousOffset: z.string(),
    note: z.string().max(2000, "noteTooLong"),
    accountId: z.string(),
    component: z.string(),
    amount: z.string(),
    currency: z.string(),
    holdingId: z.string(),
    instrumentId: z.string(),
    quantity: z.string(),
    sourceAccountId: z.string(),
    destinationAccountId: z.string(),
    sourceAmount: z.string(),
    sourceCurrency: z.string(),
    destinationAmount: z.string(),
    destinationCurrency: z.string(),
    fxRate: z.string(),
    sourceHoldingId: z.string(),
    destinationHoldingId: z.string(),
    unitPrice: z.string(),
    grossAmount: z.string(),
    settlementCurrency: z.string(),
    feeAmount: z.string(),
    confirmZeroUnitPrice: z.boolean(),
    incomeKind: z.string(),
    feeKind: z.string(),
    liabilityAccountId: z.string(),
    principalAmount: z.string(),
    principalCurrency: z.string(),
    cashAccountId: z.string(),
    cashAmount: z.string(),
    cashCurrency: z.string(),
  })
  .superRefine((value, context) => {
    function issue(path: keyof ActivityFormValues, message: string) {
      context.addIssue({ code: "custom", message, path: [path] });
    }
    function requireAmount(path: keyof ActivityFormValues, raw: string) {
      const parsed = amountField.safeParse(raw);
      if (!parsed.success) {
        issue(path, parsed.error.issues[0]?.message ?? "amount");
      }
    }
    function requireCurrency(path: keyof ActivityFormValues, raw: string) {
      const parsed = currencyField.safeParse(raw);
      if (!parsed.success) {
        issue(path, parsed.error.issues[0]?.message ?? "currency");
      }
    }
    function requireQuantity(path: keyof ActivityFormValues, raw: string) {
      const parsed = quantityField.safeParse(raw);
      if (!parsed.success) {
        issue(path, parsed.error.issues[0]?.message ?? "quantity");
      }
    }
    function requireId(path: keyof ActivityFormValues, raw: string) {
      if (raw.trim().length === 0) {
        issue(path, "required");
      }
    }

    const optionalFee = optionalAmount.safeParse(value.feeAmount);
    if (!optionalFee.success) {
      issue("feeAmount", "amount");
    }
    const optionalRate = optionalFxRate.safeParse(value.fxRate);
    if (!optionalRate.success) {
      issue("fxRate", "rate");
    }

    switch (value.kind) {
      case "deposit":
      case "withdrawal":
      case "income":
      case "fee":
      case "balance_adjustment":
      case "debt_adjustment":
      case "manual_valuation":
        requireId("accountId", value.accountId);
        requireAmount("amount", value.amount);
        requireCurrency("currency", value.currency);
        if (value.kind === "income") {
          if (
            !INCOME_KINDS.includes(value.incomeKind as (typeof INCOME_KINDS)[number])
          ) {
            issue("incomeKind", "required");
          }
        }
        if (value.kind === "fee") {
          if (!FEE_KINDS.includes(value.feeKind as (typeof FEE_KINDS)[number])) {
            issue("feeKind", "required");
          }
        }
        break;
      case "opening_adjustment":
        requireId("accountId", value.accountId);
        if (value.component === "holding_quantity") {
          requireId("holdingId", value.holdingId);
          requireId("instrumentId", value.instrumentId);
          requireQuantity("quantity", value.quantity);
        } else {
          requireAmount("amount", value.amount);
          requireCurrency("currency", value.currency);
        }
        break;
      case "position_adjustment":
      case "buy":
      case "sell":
        requireId("holdingId", value.holdingId);
        requireQuantity("quantity", value.quantity);
        if (value.kind === "buy" || value.kind === "sell") {
          const unitPrice = requiredText
            .regex(UNIT_PRICE_PATTERN, "unitPrice")
            .safeParse(value.unitPrice);
          if (!unitPrice.success) {
            issue("unitPrice", unitPrice.error.issues[0]?.message ?? "unitPrice");
          }
          requireAmount("grossAmount", value.grossAmount);
          requireCurrency("settlementCurrency", value.settlementCurrency);
        }
        break;
      case "transfer":
        if (value.transferType === "position") {
          requireId("sourceHoldingId", value.sourceHoldingId);
          requireId("destinationHoldingId", value.destinationHoldingId);
          requireQuantity("quantity", value.quantity);
        } else {
          requireId("sourceAccountId", value.sourceAccountId);
          requireId("destinationAccountId", value.destinationAccountId);
          requireAmount("sourceAmount", value.sourceAmount);
          requireCurrency("sourceCurrency", value.sourceCurrency);
          requireAmount("destinationAmount", value.destinationAmount);
          requireCurrency("destinationCurrency", value.destinationCurrency);
        }
        break;
      case "debt_draw":
      case "debt_payment":
        requireId("liabilityAccountId", value.liabilityAccountId);
        requireAmount("principalAmount", value.principalAmount);
        requireCurrency("principalCurrency", value.principalCurrency);
        if (value.kind === "debt_payment") {
          requireId("cashAccountId", value.cashAccountId);
          requireAmount("cashAmount", value.cashAmount);
          requireCurrency("cashCurrency", value.cashCurrency);
        } else if (value.cashAccountId.trim().length > 0) {
          requireAmount("cashAmount", value.cashAmount);
          requireCurrency("cashCurrency", value.cashCurrency);
        }
        break;
    }
  });

export function toCreateActivityInput(
  values: ActivityFormValues,
  accounts: AccountRecordDto[],
): CreateActivityInput {
  const shared = {
    localDate: values.localDate.trim(),
    localTime: values.localTime.trim(),
    ambiguousOffset: emptyToNull(values.ambiguousOffset),
    note: emptyToNull(values.note),
  };
  const account = accountById(accounts, values.accountId);
  const sourceAccount = accountById(accounts, values.sourceAccountId);
  const destinationAccount = accountById(accounts, values.destinationAccountId);
  const cashAccount = accountById(accounts, values.cashAccountId);
  const monetaryComponent = monetaryComponentForTrackingMode(
    account?.trackingMode ?? "balance",
  );
  const openingComponent =
    values.component.trim() ||
    (account?.trackingMode === "holdings" && values.holdingId.trim()
      ? "holding_quantity"
      : monetaryComponent);

  switch (values.kind) {
    case "opening_adjustment":
      return {
        kind: "opening_adjustment",
        ...shared,
        accountId: values.accountId.trim(),
        component: openingComponent,
        amount: openingComponent === "holding_quantity" ? null : values.amount.trim(),
        currency:
          openingComponent === "holding_quantity"
            ? null
            : values.currency.trim().toUpperCase(),
        holdingId:
          openingComponent === "holding_quantity" ? values.holdingId.trim() : null,
        instrumentId:
          openingComponent === "holding_quantity" ? values.instrumentId.trim() : null,
        quantity:
          openingComponent === "holding_quantity" ? values.quantity.trim() : null,
      };
    case "balance_adjustment":
      return {
        kind: "balance_adjustment",
        ...shared,
        accountId: values.accountId.trim(),
        amount: values.amount.trim(),
        currency: values.currency.trim().toUpperCase(),
      };
    case "position_adjustment":
      return {
        kind: "position_adjustment",
        ...shared,
        holdingId: values.holdingId.trim(),
        quantity: values.quantity.trim(),
      };
    case "deposit":
      return {
        kind: "deposit",
        ...shared,
        accountId: values.accountId.trim(),
        component: monetaryComponent,
        amount: values.amount.trim(),
        currency: values.currency.trim().toUpperCase(),
      };
    case "withdrawal":
      return {
        kind: "withdrawal",
        ...shared,
        accountId: values.accountId.trim(),
        component: monetaryComponent,
        amount: values.amount.trim(),
        currency: values.currency.trim().toUpperCase(),
      };
    case "transfer":
      if (values.transferType === "position") {
        return {
          kind: "transfer",
          ...shared,
          sourceAccountId: values.sourceAccountId.trim(),
          sourceComponent: "holding_quantity",
          sourceAmount: "0",
          sourceCurrency: "XXX",
          destinationAccountId: values.destinationAccountId.trim(),
          destinationComponent: "holding_quantity",
          destinationAmount: "0",
          destinationCurrency: "XXX",
          fxRate: null,
          sourceHoldingId: values.sourceHoldingId.trim(),
          destinationHoldingId: values.destinationHoldingId.trim(),
          quantity: values.quantity.trim(),
        };
      }
      return {
        kind: "transfer",
        ...shared,
        sourceAccountId: values.sourceAccountId.trim(),
        sourceComponent: monetaryComponentForTrackingMode(
          sourceAccount?.trackingMode ?? "balance",
        ),
        sourceAmount: values.sourceAmount.trim(),
        sourceCurrency: values.sourceCurrency.trim().toUpperCase(),
        destinationAccountId: values.destinationAccountId.trim(),
        destinationComponent: monetaryComponentForTrackingMode(
          destinationAccount?.trackingMode ?? "balance",
        ),
        destinationAmount: values.destinationAmount.trim(),
        destinationCurrency: values.destinationCurrency.trim().toUpperCase(),
        fxRate: emptyToNull(values.fxRate),
        sourceHoldingId: null,
        destinationHoldingId: null,
        quantity: null,
      };
    case "buy":
    case "sell":
      return {
        kind: values.kind,
        ...shared,
        holdingId: values.holdingId.trim(),
        quantity: values.quantity.trim(),
        unitPrice: values.unitPrice.trim(),
        grossAmount: values.grossAmount.trim(),
        settlementCurrency: values.settlementCurrency.trim().toUpperCase(),
        feeAmount: emptyToNull(values.feeAmount),
        confirmZeroUnitPrice: values.confirmZeroUnitPrice,
      };
    case "income":
      return {
        kind: "income",
        ...shared,
        accountId: values.accountId.trim(),
        component: monetaryComponent,
        amount: values.amount.trim(),
        currency: values.currency.trim().toUpperCase(),
        incomeKind: values.incomeKind,
        instrumentId: emptyToNull(values.instrumentId),
      };
    case "fee":
      return {
        kind: "fee",
        ...shared,
        accountId: values.accountId.trim(),
        component: monetaryComponent,
        amount: values.amount.trim(),
        currency: values.currency.trim().toUpperCase(),
        feeKind: values.feeKind,
        instrumentId: emptyToNull(values.instrumentId),
      };
    case "debt_draw": {
      const paired = values.cashAccountId.trim().length > 0;
      return {
        kind: "debt_draw",
        ...shared,
        liabilityAccountId: values.liabilityAccountId.trim(),
        principalAmount: values.principalAmount.trim(),
        principalCurrency: values.principalCurrency.trim().toUpperCase(),
        cashAccountId: paired ? values.cashAccountId.trim() : null,
        cashComponent: paired
          ? monetaryComponentForTrackingMode(cashAccount?.trackingMode ?? "balance")
          : null,
        cashAmount: paired ? values.cashAmount.trim() : null,
        cashCurrency: paired ? values.cashCurrency.trim().toUpperCase() : null,
        fxRate: paired ? emptyToNull(values.fxRate) : null,
      };
    }
    case "debt_payment":
      return {
        kind: "debt_payment",
        ...shared,
        liabilityAccountId: values.liabilityAccountId.trim(),
        principalAmount: values.principalAmount.trim(),
        principalCurrency: values.principalCurrency.trim().toUpperCase(),
        cashAccountId: values.cashAccountId.trim(),
        cashComponent: monetaryComponentForTrackingMode(
          cashAccount?.trackingMode ?? "balance",
        ),
        cashAmount: values.cashAmount.trim(),
        cashCurrency: values.cashCurrency.trim().toUpperCase(),
        fxRate: emptyToNull(values.fxRate),
        feeAmount: emptyToNull(values.feeAmount),
        feeKind: emptyToNull(values.feeKind),
      };
    case "debt_adjustment":
      return {
        kind: "debt_adjustment",
        ...shared,
        accountId: values.liabilityAccountId.trim() || values.accountId.trim(),
        amount: values.principalAmount.trim() || values.amount.trim(),
        currency: (
          values.principalCurrency.trim() || values.currency.trim()
        ).toUpperCase(),
      };
    case "manual_valuation":
      return {
        kind: "manual_valuation",
        ...shared,
        accountId: values.accountId.trim(),
        amount: values.amount.trim(),
        currency: values.currency.trim().toUpperCase(),
      };
  }
}

export function previewFingerprint(input: CreateActivityInput): string {
  return JSON.stringify(input);
}

export function isUserKind(kind: string): kind is UserActivityKind {
  return isUserActivityKind(kind);
}

function accountById(
  accounts: AccountRecordDto[],
  id: string,
): AccountRecordDto | undefined {
  return accounts.find((account) => account.id === id);
}
