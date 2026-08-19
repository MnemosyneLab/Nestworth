export const USER_ACTIVITY_KINDS = [
  "deposit",
  "withdrawal",
  "transfer",
  "buy",
  "sell",
  "income",
  "fee",
  "debt_draw",
  "debt_payment",
  "balance_adjustment",
  "position_adjustment",
  "debt_adjustment",
  "manual_valuation",
  "opening_adjustment",
] as const;

export type UserActivityKind = (typeof USER_ACTIVITY_KINDS)[number];

export const ACTIVITY_CLASSIFICATIONS = [
  "external_inflow",
  "external_outflow",
  "internal_transfer",
  "trade_principal",
  "income",
  "fee",
  "debt_principal",
  "remeasurement",
] as const;

export const INCOME_KINDS = [
  "salary",
  "bonus",
  "dividend",
  "interest",
  "rental",
  "pension",
  "gift",
  "refund",
  "other",
] as const;

export const FEE_KINDS = [
  "bank_fee",
  "account_fee",
  "brokerage_commission",
  "management_fee",
  "foreign_exchange_fee",
  "interest",
  "tax",
  "other",
] as const;

export const COMPONENT_KINDS = [
  "account_value",
  "holdings_cash",
  "holding_quantity",
] as const;

export const LEG_ROLES = [
  "source",
  "destination",
  "holding",
  "settlement",
  "fee",
  "income",
  "liability",
  "adjustment",
] as const;

export const LEG_DIRECTIONS = ["increase", "decrease"] as const;

export const AMBIGUOUS_OFFSETS = ["earlier", "later"] as const;

export const HISTORY_TIMEZONES = [
  "UTC",
  "Asia/Shanghai",
  "Asia/Singapore",
  "Asia/Hong_Kong",
  "Asia/Tokyo",
  "America/New_York",
  "America/Los_Angeles",
  "Europe/London",
  "Europe/Paris",
  "Australia/Sydney",
] as const;

export function isUserActivityKind(value: string): value is UserActivityKind {
  return USER_ACTIVITY_KINDS.some((kind) => kind === value);
}

export function monetaryComponentForTrackingMode(trackingMode: string): string {
  return trackingMode === "holdings" ? "holdings_cash" : "account_value";
}
