import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState, type ComponentPropsWithoutRef } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { translateAccountError } from "@/features/accounts/account-form";
import { unitPriceSchema, type UnitPriceFormValues } from "@/features/accounts/schema";
import {
  emptyInstrumentValues,
  INSTRUMENT_TYPES,
  instrumentSchema,
  instrumentToFormValues,
  toCreateInstrumentInput,
  toUpdateInstrumentInput,
  type InstrumentFormValues,
} from "@/features/instruments/schema";
import { ImagePicker } from "@/features/media/image-picker";
import { MediaImage } from "@/features/media/media-image";
import {
  applyServerFieldErrors,
  applyZodIssues,
  FieldError,
  translateReferenceError,
} from "@/features/references/form-helpers";
import {
  GhostButton,
  RecordCard,
  ReferencePage,
} from "@/features/references/reference-page";
import { freshnessLabel } from "@/features/valuation/status";
import {
  InstrumentMarketDataControls,
  useMarketDataCapabilitiesQuery,
} from "@/features/market-data/market-data-controls";
import {
  commands,
  type CommandError,
  type InstrumentQuoteRecordDto,
  type InstrumentRecordDto,
  type ReferenceCatalogDto,
} from "@/generated/tauri-bindings";
import {
  groupReferenceOptions,
  hasReferenceValue,
  legacyOptionLabel,
  referenceCatalogFromBootstrap,
  referenceCountryCodeLabel,
  referenceCurrencyCodeLabel,
  referenceGroupLabel,
  referenceSelectOptionLabel,
  withLegacyOption,
} from "@/lib/reference-catalog";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { invalidateValuation } from "@/lib/tauri/invalidate";
import { cn } from "@/lib/utils";

export function InstrumentsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [showArchived, setShowArchived] = useState(false);
  const [editor, setEditor] = useState<"create" | InstrumentRecordDto | null>(null);
  const [actionError, setActionError] = useState<CommandError | null>(null);
  const bootstrap = useBootstrapQuery();
  const catalog = referenceCatalogFromBootstrap(bootstrap.data);
  const capabilities = useMarketDataCapabilitiesQuery();
  const providerReady = Boolean(
    capabilities.data?.providers.some(
      (provider) =>
        provider.providerId === "yahoo_finance" &&
        provider.latestInstrument &&
        provider.dailyHistory,
    ),
  );

  const list = useQuery({
    queryKey: ["instruments", showArchived],
    queryFn: () =>
      unwrapResult(commands.listInstruments({ includeArchived: showArchived })),
  });

  async function invalidate() {
    await invalidateValuation(queryClient);
  }

  const archive = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.archiveInstrument({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const restore = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.restoreInstrument({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  const items = list.data ?? [];
  const listError = list.error ? commandErrorFromUnknown(list.error) : actionError;

  return (
    <ReferencePage
      addLabel={t("instruments.add")}
      empty={t("instruments.empty")}
      error={listError}
      isEmpty={!editor && items.length === 0}
      loading={list.isPending}
      onAdd={() => {
        setActionError(null);
        setEditor("create");
      }}
      onShowArchivedChange={setShowArchived}
      showArchived={showArchived}
      title={t("instruments.title")}
    >
      <div className="space-y-3">
        {editor === "create" ? (
          <InstrumentEditor
            catalog={catalog}
            providerReady={providerReady}
            onCancel={() => setEditor(null)}
            onSaved={async () => {
              setEditor(null);
              await invalidate();
            }}
          />
        ) : null}
        {items.map((instrument) =>
          editor !== "create" && editor?.id === instrument.id ? (
            <InstrumentEditor
              catalog={catalog}
              instrument={instrument}
              providerReady={providerReady}
              key={instrument.id}
              onCancel={() => setEditor(null)}
              onSaved={async () => {
                setEditor(null);
                await invalidate();
              }}
            />
          ) : (
            <RecordCard
              archived={Boolean(instrument.archivedAt)}
              details={
                <p className="mt-1 text-sm text-muted-foreground">
                  {[
                    instrument.symbol,
                    hasReferenceValue(catalog.currencies, instrument.quoteCurrency)
                      ? referenceCurrencyCodeLabel(t, catalog, instrument.quoteCurrency)
                      : legacyOptionLabel(t, instrument.quoteCurrency),
                    t(`instruments.types.${instrument.instrumentType}`, {
                      defaultValue: instrument.instrumentType,
                    }),
                    instrument.countryCode
                      ? hasReferenceValue(catalog.countries, instrument.countryCode)
                        ? referenceCountryCodeLabel(t, catalog, instrument.countryCode)
                        : legacyOptionLabel(t, instrument.countryCode)
                      : null,
                    instrument.providerKey && instrument.providerSymbol
                      ? `${t("quotes.preference.provider")}: ${instrument.providerSymbol}`
                      : null,
                  ]
                    .filter((value): value is string => Boolean(value))
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              }
              key={instrument.id}
              leading={<MediaImage alt="" assetId={instrument.logoAssetId} />}
              name={instrument.name}
            >
              <GhostButton
                onClick={() => {
                  setActionError(null);
                  setEditor(instrument);
                }}
                type="button"
              >
                {t("references.edit")}
              </GhostButton>
              {instrument.archivedAt ? (
                <GhostButton
                  aria-label={t("instruments.restoreName", { name: instrument.name })}
                  disabled={restore.isPending}
                  onClick={() => restore.mutate(instrument.id)}
                  type="button"
                >
                  {t("references.restore")}
                </GhostButton>
              ) : (
                <GhostButton
                  aria-label={t("instruments.archiveName", { name: instrument.name })}
                  disabled={archive.isPending}
                  onClick={() => archive.mutate(instrument.id)}
                  type="button"
                >
                  {t("references.archive")}
                </GhostButton>
              )}
            </RecordCard>
          ),
        )}
      </div>
    </ReferencePage>
  );
}

function InstrumentEditor({
  catalog,
  instrument,
  onCancel,
  onSaved,
  providerReady,
}: {
  catalog: ReferenceCatalogDto;
  instrument?: InstrumentRecordDto;
  onCancel: () => void;
  onSaved: () => Promise<void>;
  providerReady: boolean;
}) {
  const { t } = useTranslation();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<InstrumentFormValues>({
    defaultValues: instrument
      ? instrumentToFormValues(instrument)
      : emptyInstrumentValues,
  });
  const quotes = useQuery({
    enabled: Boolean(instrument),
    queryKey: ["instrument-quotes", instrument?.id],
    queryFn: () =>
      unwrapResult(commands.listInstrumentQuotes({ instrumentId: instrument!.id })),
  });

  const mutation = useMutation({
    mutationFn: async (values: InstrumentFormValues) => {
      if (instrument) {
        return unwrapResult(
          commands.updateInstrument(toUpdateInstrumentInput(instrument, values)),
        );
      }
      return unwrapResult(commands.createInstrument(toCreateInstrumentInput(values)));
    },
    onSuccess: onSaved,
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      applyServerFieldErrors(form, commandError.fields, [
        "name",
        "symbol",
        "instrumentType",
        "quoteCurrency",
        "marketCode",
        "countryCode",
        "isin",
        "quotePreference",
        "providerSymbol",
        "note",
      ]);
    },
  });

  return (
    <form
      className="space-y-4 rounded-xl border border-border bg-card px-4 py-4 shadow-sm"
      noValidate
      onSubmit={form.handleSubmit((values) => {
        const parsed = instrumentSchema.safeParse({
          ...values,
          quoteCurrency: values.quoteCurrency.trim().toUpperCase(),
          countryCode: values.countryCode.trim().toUpperCase(),
        });
        if (!parsed.success) {
          applyZodIssues(form, parsed.error.issues, [
            "name",
            "symbol",
            "instrumentType",
            "quoteCurrency",
            "marketCode",
            "countryCode",
            "isin",
            "quotePreference",
            "providerSymbol",
            "note",
          ]);
          return;
        }
        if (
          !instrument &&
          !hasReferenceValue(catalog.currencies, parsed.data.quoteCurrency)
        ) {
          form.setError("quoteCurrency", { type: "catalog", message: "unsupported" });
          return;
        }
        if (
          parsed.data.countryCode &&
          !hasReferenceValue(catalog.countries, parsed.data.countryCode)
        ) {
          form.setError("countryCode", { type: "catalog", message: "unsupported" });
          return;
        }
        setServerError(null);
        mutation.mutate(parsed.data);
      })}
    >
      {serverError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, serverError)}
        </p>
      ) : null}
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-name`}>
          {t("references.name")}
        </label>
        <Input
          aria-invalid={form.formState.errors.name ? true : undefined}
          autoFocus
          id={`${formId}-name`}
          {...form.register("name")}
        />
        <FieldError
          message={translateReferenceError(t, form.formState.errors.name?.message)}
        />
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-symbol`}>
            {t("instruments.symbol")}
          </label>
          <Input id={`${formId}-symbol`} {...form.register("symbol")} />
        </div>
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-type`}>
            {t("instruments.type")}
          </label>
          <NativeSelect id={`${formId}-type`} {...form.register("instrumentType")}>
            {INSTRUMENT_TYPES.map((type) => (
              <option key={type} value={type}>
                {t(`instruments.types.${type}`)}
              </option>
            ))}
          </NativeSelect>
        </div>
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-currency`}>
            {t("accounts.currency")}
          </label>
          <select
            disabled={Boolean(instrument)}
            id={`${formId}-currency`}
            className="h-10 w-full rounded-lg border border-border bg-card px-3 text-sm text-foreground outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50"
            {...form.register("quoteCurrency")}
          >
            {groupReferenceOptions(
              instrument
                ? withLegacyOption(catalog.currencies, instrument.quoteCurrency)
                : catalog.currencies,
            ).map(([group, options]) => (
              <optgroup
                key={group}
                label={referenceGroupLabel(t, "currencyGroups", group)}
              >
                {options.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.group === "legacy"
                      ? legacyOptionLabel(t, option.value)
                      : referenceSelectOptionLabel(t, "currencies", option.value)}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
          <FieldError
            message={translateAccountError(
              t,
              form.formState.errors.quoteCurrency?.message,
            )}
          />
        </div>
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-country`}>
            {t("institutions.country")}
          </label>
          <select
            id={`${formId}-country`}
            className="h-10 w-full rounded-lg border border-border bg-card px-3 text-sm text-foreground outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring"
            {...form.register("countryCode")}
          >
            <option value="">{t("accounts.none")}</option>
            {groupReferenceOptions(
              withLegacyOption(catalog.countries, instrument?.countryCode ?? ""),
            ).map(([group, options]) => (
              <optgroup
                key={group}
                label={referenceGroupLabel(t, "countryGroups", group)}
              >
                {options.map((option) => (
                  <option key={option.value} value={option.value}>
                    {option.group === "legacy"
                      ? legacyOptionLabel(t, option.value)
                      : referenceSelectOptionLabel(t, "countries", option.value)}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
          <FieldError
            message={translateReferenceError(
              t,
              form.formState.errors.countryCode?.message,
            )}
          />
        </div>
      </div>
      {providerReady || instrument?.quotePreference === "provider" ? (
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-preference`}>
            {t("quotes.preferenceLabel")}
          </label>
          <NativeSelect
            disabled={
              !providerReady && form.getValues("quotePreference") !== "provider"
            }
            id={`${formId}-preference`}
            {...form.register("quotePreference")}
          >
            <option value="manual">{t("quotes.preference.manual")}</option>
            {providerReady || instrument?.quotePreference === "provider" ? (
              <option value="provider">{t("quotes.preference.provider")}</option>
            ) : null}
          </NativeSelect>
        </div>
      ) : null}
      {form.watch("quotePreference") === "provider" ? (
        <div className="space-y-3 rounded-lg border border-border px-3 py-3">
          <div className="space-y-2">
            <label
              className="text-sm font-medium"
              htmlFor={`${formId}-provider-symbol`}
            >
              {t("marketData.symbolLabel")}
            </label>
            <Input
              id={`${formId}-provider-symbol`}
              placeholder={t("marketData.symbolPlaceholder")}
              {...form.register("providerSymbol")}
            />
            <FieldError
              message={translateReferenceError(
                t,
                form.formState.errors.providerSymbol?.message,
              )}
            />
          </div>
          <p className="text-sm text-muted-foreground">
            {t("marketData.symbolLookupUnavailable")}
          </p>
          <p className="text-sm text-muted-foreground">{t("marketData.disclaimer")}</p>
        </div>
      ) : null}
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-note`}>
          {t("references.note")}
        </label>
        <Textarea id={`${formId}-note`} {...form.register("note")} />
      </div>
      {instrument ? (
        <>
          <ImagePicker
            assetId={instrument.logoAssetId}
            entityId={instrument.id}
            kind="instrumentLogo"
            onSaved={onSaved}
          />
          <ManualPriceForm
            catalog={catalog}
            instrumentId={instrument.id}
            latest={quotes.data?.[0] ?? null}
          />
          <InstrumentMarketDataControls
            instrumentId={instrument.id}
            providerReady={
              providerReady &&
              instrument.providerKey === "yahoo_finance" &&
              Boolean(instrument.providerSymbol)
            }
          />
        </>
      ) : null}
      <div className="flex gap-2">
        <Button disabled={mutation.isPending} type="submit">
          {mutation.isPending ? t("references.saving") : t("references.save")}
        </Button>
        <GhostButton disabled={mutation.isPending} onClick={onCancel} type="button">
          {t("references.cancel")}
        </GhostButton>
      </div>
    </form>
  );
}

function ManualPriceForm({
  catalog,
  instrumentId,
  latest,
}: {
  catalog: ReferenceCatalogDto;
  instrumentId: string;
  latest: InstrumentQuoteRecordDto | null;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<UnitPriceFormValues>({ defaultValues: { unitPrice: "" } });
  const mutation = useMutation({
    mutationFn: async (values: UnitPriceFormValues) =>
      unwrapResult(
        commands.appendManualInstrumentQuote({
          instrumentId,
          unitPrice: values.unitPrice.trim(),
          quotedAt: null,
        }),
      ),
    onSuccess: async () => {
      form.reset({ unitPrice: "" });
      await invalidateValuation(queryClient);
      await queryClient.invalidateQueries({
        queryKey: ["instrument-quotes", instrumentId],
      });
    },
    onError: (error) => setServerError(commandErrorFromUnknown(error)),
  });

  return (
    <div className="space-y-3 rounded-lg border border-border px-3 py-3">
      <h3 className="text-sm font-medium">{t("quotes.addPrice")}</h3>
      {latest ? (
        <p className="text-sm text-muted-foreground">
          {latest.unitPrice}{" "}
          {referenceCurrencyCodeLabel(t, catalog, latest.quoteCurrency)} ·{" "}
          {freshnessLabel(t, latest.sourceKind)}
        </p>
      ) : (
        <p className="text-sm text-muted-foreground">{t("quotes.noPrice")}</p>
      )}
      {serverError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, serverError)}
        </p>
      ) : null}
      <label className="grid gap-1 text-sm" htmlFor={`${formId}-price`}>
        {t("quotes.unitPrice")}
        <Input
          id={`${formId}-price`}
          inputMode="decimal"
          type="text"
          {...form.register("unitPrice")}
        />
      </label>
      <FieldError
        message={translateAccountError(t, form.formState.errors.unitPrice?.message)}
      />
      <Button
        disabled={mutation.isPending}
        onClick={form.handleSubmit((values) => {
          const parsed = unitPriceSchema.safeParse(values);
          if (!parsed.success) {
            applyZodIssues(form, parsed.error.issues, ["unitPrice"]);
            return;
          }
          setServerError(null);
          mutation.mutate(parsed.data);
        })}
        type="button"
      >
        {mutation.isPending ? t("references.saving") : t("references.save")}
      </Button>
    </div>
  );
}

function NativeSelect({ className, ...props }: ComponentPropsWithoutRef<"select">) {
  return (
    <select
      {...props}
      className={cn(
        "h-10 w-full rounded-lg border border-border bg-card px-3 text-sm text-foreground shadow-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
    />
  );
}

export default InstrumentsPage;
