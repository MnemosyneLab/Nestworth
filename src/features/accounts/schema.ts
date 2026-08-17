import { z } from "zod";

import type {
  AccountRecordDto,
  CreateAccountInput,
  OwnershipShareInput,
  UpdateAccountInput,
} from "@/generated/tauri-bindings";
import { emptyToNull } from "@/lib/empty-to-null";

export const TOTAL_BPS = 10_000;

export const AMOUNT_PATTERN = /^(0|[1-9][0-9]{0,11})(\.[0-9]{1,4})?$/;
export const DATE_PATTERN = /^\d{4}-\d{2}-\d{2}$/;

export const SECONDARY_CATEGORIES = [
  { primary: "cash_equivalent", secondary: "cash" },
  { primary: "cash_equivalent", secondary: "bank_account" },
  { primary: "cash_equivalent", secondary: "digital_wallet" },
  { primary: "cash_equivalent", secondary: "broker_cash" },
  { primary: "cash_equivalent", secondary: "other_cash_equivalent" },
  { primary: "investment", secondary: "brokerage_account" },
  { primary: "investment", secondary: "investment_fund_account" },
  { primary: "investment", secondary: "bank_investment_product" },
  { primary: "investment", secondary: "insurance" },
  { primary: "investment", secondary: "manual_investment" },
  { primary: "investment", secondary: "other_investment" },
  { primary: "property", secondary: "real_estate" },
  { primary: "property", secondary: "vehicle" },
  { primary: "property", secondary: "collectible" },
  { primary: "property", secondary: "other_property" },
  { primary: "receivable", secondary: "loan_receivable" },
  { primary: "receivable", secondary: "other_receivable" },
  { primary: "liability", secondary: "credit_card" },
  { primary: "liability", secondary: "mortgage" },
  { primary: "liability", secondary: "auto_loan" },
  { primary: "liability", secondary: "consumer_loan" },
  { primary: "liability", secondary: "personal_debt" },
  { primary: "liability", secondary: "other_liability" },
] as const;

export const PRIMARY_CATEGORIES = [
  "cash_equivalent",
  "investment",
  "property",
  "receivable",
  "liability",
] as const;

export type AccountFormValues = {
  name: string;
  secondaryCategory: string;
  institutionId: string;
  groupId: string;
  note: string;
  includeInNetWorth: boolean;
  includeInInvestment: boolean;
  includeInLiquidAssets: boolean;
  openedOn: string;
  closedOn: string;
  owners: Array<{ memberId: string; percent: string }>;
  initialAmount: string;
};

export type CategoryDefaults = {
  primaryCategory: string;
  trackingMode: "balance" | "manual_value";
  includeInNetWorth: boolean;
  includeInInvestment: boolean;
  includeInLiquidAssets: boolean;
};

export function categoryDefaults(secondary: string): CategoryDefaults {
  const primary =
    SECONDARY_CATEGORIES.find((category) => category.secondary === secondary)
      ?.primary ?? "cash_equivalent";
  return {
    primaryCategory: primary,
    trackingMode:
      primary === "cash_equivalent" || primary === "liability"
        ? "balance"
        : "manual_value",
    includeInNetWorth: true,
    includeInInvestment: primary === "investment",
    includeInLiquidAssets: primary === "cash_equivalent",
  };
}

export function percentToBasisPoints(percent: string): number | null {
  if (percent.length === 0 || /[^0-9.]/.test(percent)) {
    return null;
  }

  const parts = percent.split(".");
  if (parts.length > 2) {
    return null;
  }
  const integer = parts[0];
  const fraction = parts[1];

  if (fraction !== undefined) {
    if (fraction.length === 0 || fraction.length > 2 || /[^0-9]/.test(fraction)) {
      return null;
    }
  }
  if (
    integer.length === 0 ||
    (integer !== "0" && integer.startsWith("0")) ||
    /[^0-9]/.test(integer)
  ) {
    return null;
  }

  const integerValue = Number.parseInt(integer, 10);
  if (!Number.isInteger(integerValue) || integerValue > 100) {
    return null;
  }

  let fractionValue = 0;
  if (fraction !== undefined) {
    const parsed = Number.parseInt(fraction, 10);
    if (!Number.isInteger(parsed)) {
      return null;
    }
    fractionValue = fraction.length === 1 ? parsed * 10 : parsed;
  }

  if (integerValue === 100 && fractionValue !== 0) {
    return null;
  }

  const basisPoints = integerValue * 100 + fractionValue;
  if (basisPoints < 1 || basisPoints > TOTAL_BPS) {
    return null;
  }
  return basisPoints;
}

export function basisPointsToPercent(basisPoints: number): string {
  const whole = Math.trunc(basisPoints / 100);
  const fraction = basisPoints % 100;
  if (fraction === 0) {
    return String(whole);
  }
  if (fraction % 10 === 0) {
    return `${whole}.${fraction / 10}`;
  }
  return `${whole}.${String(fraction).padStart(2, "0")}`;
}

export function equalSplitPercents(count: number): string[] {
  if (count < 1) {
    return [];
  }
  const base = Math.trunc(TOTAL_BPS / count);
  const remainder = TOTAL_BPS % count;
  return Array.from({ length: count }, (_, index) =>
    basisPointsToPercent(base + (index < remainder ? 1 : 0)),
  );
}

export function formatMoneyAmount(amount: string): string {
  const negative = amount.startsWith("-");
  const unsigned = negative ? amount.slice(1) : amount;
  const [integer, fraction] = unsigned.split(".");
  const grouped = integer.replace(/\B(?=(\d{3})+(?!\d))/g, ",");
  const body = fraction === undefined ? grouped : `${grouped}.${fraction}`;
  return negative ? `-${body}` : body;
}

export function formatMoney(amount: string, currency: string): string {
  const formatted = formatMoneyAmount(amount);
  if (formatted.startsWith("-")) {
    return `${currency} -${formatted.slice(1)}`;
  }
  return `${currency} ${formatted}`;
}

function optionalDate(value: string) {
  return value === "" || DATE_PATTERN.test(value);
}

const ownerSchema = z.object({
  memberId: z.string().trim().min(1, "required"),
  percent: z
    .string()
    .trim()
    .min(1, "required")
    .refine((value) => percentToBasisPoints(value) !== null, "percent"),
});

export const accountSchema = z
  .object({
    name: z.string().trim().min(1, "required").max(80, "tooLong"),
    secondaryCategory: z
      .string()
      .refine(
        (value) =>
          SECONDARY_CATEGORIES.some((category) => category.secondary === value),
        "required",
      ),
    institutionId: z.string(),
    groupId: z.string(),
    note: z.string().max(2000, "noteTooLong"),
    includeInNetWorth: z.boolean(),
    includeInInvestment: z.boolean(),
    includeInLiquidAssets: z.boolean(),
    openedOn: z.string().refine(optionalDate, "date"),
    closedOn: z.string().refine(optionalDate, "date"),
    owners: z.array(ownerSchema).min(1, "owners"),
    initialAmount: z.string(),
  })
  .superRefine((value, context) => {
    const memberIds = value.owners.map((owner) => owner.memberId);
    if (new Set(memberIds).size !== memberIds.length) {
      context.addIssue({
        code: "custom",
        message: "ownerDuplicate",
        path: ["owners"],
      });
    }
    const totals = value.owners.map((owner) =>
      percentToBasisPoints(owner.percent.trim()),
    );
    if (totals.every((item) => item !== null)) {
      const total = totals.reduce<number>((sum, item) => sum + item, 0);
      if (total !== TOTAL_BPS) {
        context.addIssue({
          code: "custom",
          message: "ownersTotal",
          path: ["owners"],
        });
      }
    }
    if (value.openedOn && value.closedOn && value.closedOn < value.openedOn) {
      context.addIssue({
        code: "custom",
        message: "closedOn",
        path: ["closedOn"],
      });
    }
  });

export const createAccountSchema = accountSchema.superRefine((value, context) => {
  if (!AMOUNT_PATTERN.test(value.initialAmount.trim())) {
    context.addIssue({
      code: "custom",
      message: "amount",
      path: ["initialAmount"],
    });
  }
});

export const updateValueSchema = z.object({
  amount: z.string().trim().min(1, "required").regex(AMOUNT_PATTERN, "amount"),
});

export type UpdateValueFormValues = z.infer<typeof updateValueSchema>;

export function emptyAccountValues(memberId: string): AccountFormValues {
  const defaults = categoryDefaults("bank_account");
  return {
    name: "",
    secondaryCategory: "bank_account",
    institutionId: "",
    groupId: "",
    note: "",
    includeInNetWorth: defaults.includeInNetWorth,
    includeInInvestment: defaults.includeInInvestment,
    includeInLiquidAssets: defaults.includeInLiquidAssets,
    openedOn: "",
    closedOn: "",
    owners: [{ memberId, percent: "100" }],
    initialAmount: "",
  };
}

export function accountToFormValues(account: AccountRecordDto): AccountFormValues {
  return {
    name: account.name,
    secondaryCategory: account.secondaryCategory,
    institutionId: account.institutionId ?? "",
    groupId: account.groupId ?? "",
    note: account.note ?? "",
    includeInNetWorth: account.includeInNetWorth,
    includeInInvestment: account.includeInInvestment,
    includeInLiquidAssets: account.includeInLiquidAssets,
    openedOn: account.openedOn ?? "",
    closedOn: account.closedOn ?? "",
    owners: account.owners.map((owner) => ({
      memberId: owner.memberId,
      percent: basisPointsToPercent(owner.shareBps),
    })),
    initialAmount: "",
  };
}

function ownerInputs(owners: AccountFormValues["owners"]): OwnershipShareInput[] {
  return owners.map((owner) => ({
    memberId: owner.memberId,
    percent: null,
    shareBps: percentToBasisPoints(owner.percent.trim()) ?? 0,
  }));
}

export function toCreateAccountInput(
  values: AccountFormValues,
  defaultCurrency: string,
): CreateAccountInput {
  const defaults = categoryDefaults(values.secondaryCategory);
  return {
    name: values.name.trim(),
    primaryCategory: defaults.primaryCategory,
    secondaryCategory: values.secondaryCategory,
    defaultCurrency,
    institutionId: emptyToNull(values.institutionId),
    groupId: emptyToNull(values.groupId),
    trackingMode: defaults.trackingMode,
    note: emptyToNull(values.note),
    includeInNetWorth: values.includeInNetWorth,
    includeInInvestment: values.includeInInvestment,
    includeInLiquidAssets: values.includeInLiquidAssets,
    openedOn: emptyToNull(values.openedOn),
    closedOn: emptyToNull(values.closedOn),
    owners: ownerInputs(values.owners),
    initialAmount: values.initialAmount.trim(),
  };
}

export function toUpdateAccountInput(
  id: string,
  values: AccountFormValues,
): UpdateAccountInput {
  const defaults = categoryDefaults(values.secondaryCategory);
  return {
    id,
    name: values.name.trim(),
    primaryCategory: defaults.primaryCategory,
    secondaryCategory: values.secondaryCategory,
    institutionId: emptyToNull(values.institutionId),
    groupId: emptyToNull(values.groupId),
    trackingMode: defaults.trackingMode,
    note: emptyToNull(values.note),
    includeInNetWorth: values.includeInNetWorth,
    includeInInvestment: values.includeInInvestment,
    includeInLiquidAssets: values.includeInLiquidAssets,
    openedOn: emptyToNull(values.openedOn),
    closedOn: emptyToNull(values.closedOn),
    owners: ownerInputs(values.owners),
  };
}
