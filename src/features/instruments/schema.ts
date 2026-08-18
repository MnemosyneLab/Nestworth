import { z } from "zod";

import { CURRENCY_PATTERN } from "@/features/accounts/schema";
import type {
  CreateInstrumentInput,
  InstrumentRecordDto,
  UpdateInstrumentInput,
} from "@/generated/tauri-bindings";
import { emptyToNull } from "@/lib/empty-to-null";

export const INSTRUMENT_TYPES = [
  "stock",
  "etf",
  "mutual_fund",
  "crypto",
  "bond",
  "precious_metal",
  "bank_investment_product",
  "other",
] as const;

export const instrumentSchema = z.object({
  name: z.string().trim().min(1, "required").max(80, "tooLong"),
  symbol: z.string().max(80, "tooLong"),
  instrumentType: z
    .string()
    .refine(
      (value) => INSTRUMENT_TYPES.includes(value as (typeof INSTRUMENT_TYPES)[number]),
      "required",
    ),
  quoteCurrency: z.string().trim().regex(CURRENCY_PATTERN, "currency"),
  marketCode: z.string().max(80, "tooLong"),
  countryCode: z
    .string()
    .trim()
    .refine((value) => value === "" || /^[A-Z]{2}$/.test(value), "country"),
  isin: z.string().max(80, "tooLong"),
  quotePreference: z.enum(["manual", "provider"]),
  note: z.string().max(2000, "noteTooLong"),
});

export type InstrumentFormValues = z.infer<typeof instrumentSchema>;

export const emptyInstrumentValues: InstrumentFormValues = {
  name: "",
  symbol: "",
  instrumentType: "etf",
  quoteCurrency: "USD",
  marketCode: "",
  countryCode: "",
  isin: "",
  quotePreference: "manual",
  note: "",
};

export function instrumentToFormValues(
  instrument: InstrumentRecordDto,
): InstrumentFormValues {
  return {
    name: instrument.name,
    symbol: instrument.symbol ?? "",
    instrumentType: instrument.instrumentType,
    quoteCurrency: instrument.quoteCurrency,
    marketCode: instrument.marketCode ?? "",
    countryCode: instrument.countryCode ?? "",
    isin: instrument.isin ?? "",
    quotePreference: instrument.quotePreference === "provider" ? "provider" : "manual",
    note: instrument.note ?? "",
  };
}

export function toCreateInstrumentInput(
  values: InstrumentFormValues,
): CreateInstrumentInput {
  return {
    name: values.name.trim(),
    symbol: emptyToNull(values.symbol),
    instrumentType: values.instrumentType,
    quoteCurrency: values.quoteCurrency.trim(),
    marketCode: emptyToNull(values.marketCode),
    countryCode: emptyToNull(values.countryCode.trim().toUpperCase()),
    isin: emptyToNull(values.isin),
    providerKey: null,
    providerSymbol: null,
    quotePreference: "manual",
    note: emptyToNull(values.note),
  };
}

export function toUpdateInstrumentInput(
  instrument: InstrumentRecordDto,
  values: InstrumentFormValues,
): UpdateInstrumentInput {
  return {
    id: instrument.id,
    name: values.name.trim(),
    symbol: emptyToNull(values.symbol),
    instrumentType: values.instrumentType,
    marketCode: emptyToNull(values.marketCode),
    countryCode: emptyToNull(values.countryCode.trim().toUpperCase()),
    isin: emptyToNull(values.isin),
    providerKey: instrument.providerKey,
    providerSymbol: instrument.providerSymbol,
    quotePreference: values.quotePreference,
    note: emptyToNull(values.note),
  };
}
