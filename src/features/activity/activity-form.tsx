import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useMemo, useRef, useState, type ComponentPropsWithoutRef } from "react";
import { useForm, type Path } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { translateAccountError } from "@/features/accounts/account-form";
import { ActivityPreviewPanel } from "@/features/activity/activity-display";
import {
  AMBIGUOUS_OFFSETS,
  FEE_KINDS,
  HISTORY_TIMEZONES,
  INCOME_KINDS,
  USER_ACTIVITY_KINDS,
  type UserActivityKind,
} from "@/features/activity/kinds";
import {
  ACTIVITY_FORM_FIELDS,
  activityFormSchema,
  emptyActivityFormValues,
  previewFingerprint,
  toCreateActivityInput,
  type ActivityFormValues,
} from "@/features/activity/schema";
import {
  applyServerFieldErrors,
  applyZodIssues,
  FieldError,
} from "@/features/references/form-helpers";
import { GhostButton } from "@/features/references/reference-page";
import {
  commands,
  type AccountRecordDto,
  type ActivityPreviewDto,
  type CommandError,
  type CreateActivityInput,
  type HistoryOriginDto,
  type HoldingRecordDto,
  type InstrumentRecordDto,
  type ReferenceCatalogDto,
} from "@/generated/tauri-bindings";
import {
  groupReferenceOptions,
  referenceGroupLabel,
  referenceSelectOptionLabel,
} from "@/lib/reference-catalog";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { invalidateValuation } from "@/lib/tauri/invalidate";
import { cn } from "@/lib/utils";

export function translateActivityError(
  t: ReturnType<typeof useTranslation>["t"],
  message: string | undefined,
): string | undefined {
  const translated = translateAccountError(t, message);
  if (!message || translated !== message) {
    return translated;
  }
  if (message === "time") {
    return t("activity.errors.time");
  }
  return message;
}

export function TimezoneConfirmation({
  origin,
  onConfirmed,
}: {
  origin: HistoryOriginDto;
  onConfirmed: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const formId = useId();
  const [timezone, setTimezone] = useState(origin.timezone || "UTC");
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const confirm = useMutation({
    mutationFn: (next: string) =>
      unwrapResult(commands.confirmHistoryTimezone({ timezone: next })),
    onSuccess: onConfirmed,
    onError: (error) => setServerError(commandErrorFromUnknown(error)),
  });

  return (
    <section
      aria-labelledby={`${formId}-title`}
      className="space-y-3 rounded-xl border border-border bg-card px-4 py-4 shadow-sm"
    >
      <h2 className="text-lg font-medium" id={`${formId}-title`}>
        {t("activity.timezoneTitle")}
      </h2>
      <p className="text-sm text-muted-foreground">
        {t("activity.timezoneDescription")}
      </p>
      <p className="text-sm text-muted-foreground">
        {t("activity.timezoneCurrent", { timezone: origin.timezone })}
      </p>
      {serverError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, serverError)}
        </p>
      ) : null}
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-timezone`}>
          {t("activity.timezoneField")}
        </label>
        <NativeSelect
          id={`${formId}-timezone`}
          onChange={(event) => setTimezone(event.target.value)}
          value={
            HISTORY_TIMEZONES.includes(timezone as (typeof HISTORY_TIMEZONES)[number])
              ? timezone
              : "UTC"
          }
        >
          {HISTORY_TIMEZONES.map((value) => (
            <option key={value} value={value}>
              {value}
            </option>
          ))}
        </NativeSelect>
        <Input
          aria-label={t("activity.timezoneField")}
          onChange={(event) => setTimezone(event.target.value)}
          value={timezone}
        />
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          disabled={confirm.isPending}
          onClick={() => {
            setServerError(null);
            confirm.mutate("UTC");
          }}
          type="button"
          variant="ghost"
        >
          {t("activity.timezoneUtc")}
        </Button>
        <Button
          disabled={confirm.isPending || timezone.trim().length === 0}
          onClick={() => {
            setServerError(null);
            confirm.mutate(timezone.trim());
          }}
          type="button"
        >
          {confirm.isPending
            ? t("activity.timezoneConfirming")
            : t("activity.timezoneConfirm")}
        </Button>
      </div>
    </section>
  );
}

export function ActivityForm({
  accounts,
  catalog,
  defaultCurrency,
  instruments,
  mode,
  originalId,
  timezoneConfirmed,
  onCancel,
  onPosted,
}: {
  accounts: AccountRecordDto[];
  catalog: ReferenceCatalogDto;
  defaultCurrency: string;
  instruments: InstrumentRecordDto[];
  mode: "create" | "correction";
  originalId?: string;
  timezoneConfirmed: boolean;
  onCancel: () => void;
  onPosted: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const [preview, setPreview] = useState<ActivityPreviewDto | null>(null);
  const [previewedKey, setPreviewedKey] = useState<string | null>(null);
  const [confirmingCorrection, setConfirmingCorrection] = useState(false);
  const postButtonRef = useRef<HTMLButtonElement>(null);
  const now = useMemo(() => currentLocalDateTime(), []);
  const form = useForm<ActivityFormValues>({
    defaultValues: emptyActivityFormValues(
      "deposit",
      defaultCurrency,
      now.date,
      now.time,
    ),
  });
  const kind = form.watch("kind");
  const transferType = form.watch("transferType");
  const accountId = form.watch("accountId");
  const sourceAccountId = form.watch("sourceAccountId");
  const destinationAccountId = form.watch("destinationAccountId");
  const holdingAccountId =
    kind === "buy" || kind === "sell" || kind === "position_adjustment"
      ? accountId
      : "";

  const holdings = useHoldings(holdingAccountId);
  const sourceHoldings = useHoldings(
    kind === "transfer" && transferType === "position" ? sourceAccountId : "",
  );
  const destinationHoldings = useHoldings(
    kind === "transfer" && transferType === "position" ? destinationAccountId : "",
  );

  const currentInput = (): CreateActivityInput | null => {
    const parsed = activityFormSchema.safeParse(form.getValues());
    if (!parsed.success) {
      applyZodIssues(
        form,
        parsed.error.issues,
        ACTIVITY_FORM_FIELDS as Array<Path<ActivityFormValues>>,
      );
      return null;
    }
    return toCreateActivityInput(parsed.data, accounts);
  };

  const currentKey = previewFingerprint(toCreateActivityInput(form.watch(), accounts));
  const previewStale = Boolean(preview && previewedKey && currentKey !== previewedKey);

  const previewMutation = useMutation({
    mutationFn: async (input: CreateActivityInput) =>
      unwrapResult(commands.previewActivity(input)),
    onSuccess: (data, input) => {
      setPreview(data);
      setPreviewedKey(previewFingerprint(input));
    },
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      applyServerFieldErrors(
        form,
        commandError.fields,
        ACTIVITY_FORM_FIELDS as Array<Path<ActivityFormValues>>,
      );
    },
  });

  const createMutation = useMutation({
    mutationFn: async (input: CreateActivityInput) => {
      if (mode === "correction") {
        if (!originalId) {
          throw commandErrorFromUnknown(new Error("missing original"));
        }
        return unwrapResult(
          commands.correctActivity({ originalId, replacement: input }),
        );
      }
      return unwrapResult(commands.createActivity(input));
    },
    onSuccess: async () => {
      await invalidateValuation(queryClient);
      await onPosted();
    },
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      applyServerFieldErrors(
        form,
        commandError.fields,
        ACTIVITY_FORM_FIELDS as Array<Path<ActivityFormValues>>,
      );
    },
  });

  const pending = previewMutation.isPending || createMutation.isPending;
  const canPost = Boolean(preview) && !previewStale && !pending && timezoneConfirmed;

  function runPreview() {
    setServerError(null);
    form.clearErrors();
    const input = currentInput();
    if (!input || !timezoneConfirmed) {
      return;
    }
    previewMutation.mutate(input);
  }

  function runPost() {
    setServerError(null);
    const input = currentInput();
    if (!input || !timezoneConfirmed) {
      return;
    }
    if (!preview || previewFingerprint(input) !== previewedKey) {
      setPreview(null);
      setPreviewedKey(null);
      return;
    }
    if (mode === "correction" && !confirmingCorrection) {
      setConfirmingCorrection(true);
      return;
    }
    createMutation.mutate(input);
  }

  return (
    <form
      className="space-y-4 rounded-xl border border-border bg-card px-4 py-4 shadow-sm"
      noValidate
      onSubmit={(event) => {
        event.preventDefault();
        runPost();
      }}
    >
      <h2 className="text-lg font-medium">
        {mode === "correction" ? t("activity.correctionForm") : t("activity.add")}
      </h2>
      <p className="text-sm text-muted-foreground">{t(`activity.help.${kind}`)}</p>
      {serverError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, serverError)}
        </p>
      ) : null}
      {previewStale ? (
        <p className="text-sm text-destructive" role="alert">
          {t("activity.previewStale")}
        </p>
      ) : null}
      <div className="grid gap-3 sm:grid-cols-2">
        <LabeledSelect
          id={`${formId}-kind`}
          label={t("activity.kind")}
          {...form.register("kind")}
        >
          {USER_ACTIVITY_KINDS.map((value) => (
            <option key={value} value={value}>
              {t(`activity.kinds.${value}`)}
            </option>
          ))}
        </LabeledSelect>
        <Field
          error={form.formState.errors.localDate?.message}
          id={`${formId}-date`}
          label={t("activity.localDate")}
        >
          <Input id={`${formId}-date`} type="text" {...form.register("localDate")} />
        </Field>
        <Field
          error={form.formState.errors.localTime?.message}
          id={`${formId}-time`}
          label={t("activity.localTime")}
        >
          <Input id={`${formId}-time`} type="text" {...form.register("localTime")} />
        </Field>
        <LabeledSelect
          id={`${formId}-offset`}
          label={t("activity.ambiguousOffset")}
          {...form.register("ambiguousOffset")}
        >
          <option value="">{t("activity.ambiguousNone")}</option>
          {AMBIGUOUS_OFFSETS.map((value) => (
            <option key={value} value={value}>
              {t(
                value === "earlier"
                  ? "activity.ambiguousEarlier"
                  : "activity.ambiguousLater",
              )}
            </option>
          ))}
        </LabeledSelect>
      </div>
      {kind === "transfer" ? (
        <LabeledSelect
          id={`${formId}-transfer-type`}
          label={t("activity.transferType")}
          {...form.register("transferType")}
        >
          <option value="cash">{t("activity.transferCash")}</option>
          <option value="position">{t("activity.transferPosition")}</option>
        </LabeledSelect>
      ) : null}
      <KindFields
        accounts={accounts}
        catalog={catalog}
        formId={formId}
        destinationHoldings={destinationHoldings.data ?? []}
        holdings={holdings.data ?? []}
        instruments={instruments}
        kind={kind}
        register={form.register}
        sourceHoldings={sourceHoldings.data ?? []}
        transferType={transferType}
        errors={form.formState.errors}
      />
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-note`}>
          {t("references.note")}
        </label>
        <Textarea id={`${formId}-note`} {...form.register("note")} />
        <FieldError
          message={translateActivityError(t, form.formState.errors.note?.message)}
        />
      </div>
      {preview && !previewStale ? (
        <ActivityPreviewPanel catalog={catalog} preview={preview} />
      ) : null}
      {mode === "correction" && confirmingCorrection ? (
        <div
          className="space-y-3 rounded-lg border border-border px-4 py-3"
          role="group"
        >
          <p className="font-medium">{t("activity.confirmCorrectTitle")}</p>
          <p className="text-sm text-muted-foreground">
            {t("activity.confirmCorrectDescription")}
          </p>
          <div className="flex flex-wrap gap-2">
            <Button autoFocus disabled={pending} onClick={runPost} type="button">
              {createMutation.isPending
                ? t("activity.correcting")
                : t("activity.confirmCorrect")}
            </Button>
            <GhostButton
              disabled={pending}
              onClick={() => {
                setConfirmingCorrection(false);
                requestAnimationFrame(() => postButtonRef.current?.focus());
              }}
              type="button"
            >
              {t("references.cancel")}
            </GhostButton>
          </div>
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          <Button
            disabled={pending || !timezoneConfirmed}
            onClick={runPreview}
            type="button"
            variant="ghost"
          >
            {previewMutation.isPending
              ? t("activity.previewing")
              : t("activity.preview")}
          </Button>
          <Button disabled={!canPost} ref={postButtonRef} type="submit">
            {createMutation.isPending
              ? mode === "correction"
                ? t("activity.correcting")
                : t("activity.posting")
              : mode === "correction"
                ? t("activity.correct")
                : t("activity.post")}
          </Button>
          <GhostButton disabled={pending} onClick={onCancel} type="button">
            {t("references.cancel")}
          </GhostButton>
        </div>
      )}
    </form>
  );
}

function KindFields({
  accounts,
  catalog,
  destinationHoldings,
  errors,
  formId,
  holdings,
  instruments,
  kind,
  register,
  sourceHoldings,
  transferType,
}: {
  accounts: AccountRecordDto[];
  catalog: ReferenceCatalogDto;
  destinationHoldings: HoldingRecordDto[];
  errors: ReturnType<typeof useForm<ActivityFormValues>>["formState"]["errors"];
  formId: string;
  holdings: HoldingRecordDto[];
  instruments: InstrumentRecordDto[];
  kind: UserActivityKind;
  register: ReturnType<typeof useForm<ActivityFormValues>>["register"];
  sourceHoldings: HoldingRecordDto[];
  transferType: "cash" | "position";
}) {
  const { t } = useTranslation();
  const showAccount = [
    "deposit",
    "withdrawal",
    "income",
    "fee",
    "balance_adjustment",
    "manual_valuation",
    "opening_adjustment",
    "buy",
    "sell",
    "position_adjustment",
  ].includes(kind);
  const showAmount = [
    "deposit",
    "withdrawal",
    "income",
    "fee",
    "balance_adjustment",
    "manual_valuation",
    "opening_adjustment",
  ].includes(kind);
  const showHolding =
    kind === "buy" || kind === "sell" || kind === "position_adjustment";
  const showTrade = kind === "buy" || kind === "sell";

  return (
    <div className="grid gap-3 sm:grid-cols-2">
      {showAccount ? (
        <AccountSelect
          accounts={accounts}
          id={`${formId}-account`}
          label={t("activity.account")}
          error={errors.accountId?.message}
          {...register("accountId")}
        />
      ) : null}
      {showHolding ? (
        <HoldingSelect
          error={errors.holdingId?.message}
          holdings={holdings}
          id={`${formId}-holding`}
          label={t("activity.holding")}
          {...register("holdingId")}
        />
      ) : null}
      {showAmount ? (
        <>
          <Field
            error={errors.amount?.message}
            id={`${formId}-amount`}
            label={t("accounts.amount")}
          >
            <Input
              id={`${formId}-amount`}
              inputMode="decimal"
              {...register("amount")}
            />
          </Field>
          <CurrencySelect
            catalog={catalog}
            error={errors.currency?.message}
            id={`${formId}-currency`}
            label={t("accounts.currency")}
            {...register("currency")}
          />
        </>
      ) : null}
      {kind === "transfer" && transferType === "cash" ? (
        <>
          <AccountSelect
            accounts={accounts}
            error={errors.sourceAccountId?.message}
            id={`${formId}-source`}
            label={t("activity.sourceAccount")}
            {...register("sourceAccountId")}
          />
          <AccountSelect
            accounts={accounts}
            error={errors.destinationAccountId?.message}
            id={`${formId}-destination`}
            label={t("activity.destinationAccount")}
            {...register("destinationAccountId")}
          />
          <Field
            error={errors.sourceAmount?.message}
            id={`${formId}-source-amount`}
            label={t("activity.sourceAmount")}
          >
            <Input
              id={`${formId}-source-amount`}
              inputMode="decimal"
              {...register("sourceAmount")}
            />
          </Field>
          <CurrencySelect
            catalog={catalog}
            error={errors.sourceCurrency?.message}
            id={`${formId}-source-currency`}
            label={t("activity.sourceCurrency")}
            {...register("sourceCurrency")}
          />
          <Field
            error={errors.destinationAmount?.message}
            id={`${formId}-dest-amount`}
            label={t("activity.destinationAmount")}
          >
            <Input
              id={`${formId}-dest-amount`}
              inputMode="decimal"
              {...register("destinationAmount")}
            />
          </Field>
          <CurrencySelect
            catalog={catalog}
            error={errors.destinationCurrency?.message}
            id={`${formId}-dest-currency`}
            label={t("activity.destinationCurrency")}
            {...register("destinationCurrency")}
          />
          <Field
            error={errors.feeAmount?.message}
            id={`${formId}-transfer-fee`}
            label={t("activity.feeAmount")}
          >
            <Input
              id={`${formId}-transfer-fee`}
              inputMode="decimal"
              {...register("feeAmount")}
            />
          </Field>
        </>
      ) : null}
      {kind === "transfer" && transferType === "position" ? (
        <>
          <AccountSelect
            accounts={accounts.filter((account) => account.trackingMode === "holdings")}
            error={errors.sourceAccountId?.message}
            id={`${formId}-source-account`}
            label={t("activity.sourceAccount")}
            {...register("sourceAccountId")}
          />
          <HoldingSelect
            error={errors.sourceHoldingId?.message}
            holdings={sourceHoldings}
            id={`${formId}-source-holding`}
            label={t("activity.sourceHolding")}
            {...register("sourceHoldingId")}
          />
          <AccountSelect
            accounts={accounts.filter((account) => account.trackingMode === "holdings")}
            error={errors.destinationAccountId?.message}
            id={`${formId}-dest-account`}
            label={t("activity.destinationAccount")}
            {...register("destinationAccountId")}
          />
          <HoldingSelect
            error={errors.destinationHoldingId?.message}
            holdings={destinationHoldings}
            id={`${formId}-dest-holding`}
            label={t("activity.destinationHolding")}
            {...register("destinationHoldingId")}
          />
          <Field
            error={errors.quantity?.message}
            id={`${formId}-quantity`}
            label={t("accounts.quantity")}
          >
            <Input
              id={`${formId}-quantity`}
              inputMode="decimal"
              {...register("quantity")}
            />
          </Field>
        </>
      ) : null}
      {showTrade || kind === "position_adjustment" || kind === "opening_adjustment" ? (
        kind === "opening_adjustment" ? null : (
          <Field
            error={errors.quantity?.message}
            id={`${formId}-qty`}
            label={t("accounts.quantity")}
          >
            <Input id={`${formId}-qty`} inputMode="decimal" {...register("quantity")} />
          </Field>
        )
      ) : null}
      {showTrade ? (
        <>
          <Field
            error={errors.unitPrice?.message}
            id={`${formId}-unit-price`}
            label={t("activity.unitPrice")}
          >
            <Input
              id={`${formId}-unit-price`}
              inputMode="decimal"
              {...register("unitPrice")}
            />
          </Field>
          <Field
            error={errors.grossAmount?.message}
            id={`${formId}-gross`}
            label={t("activity.grossAmount")}
          >
            <Input
              id={`${formId}-gross`}
              inputMode="decimal"
              {...register("grossAmount")}
            />
          </Field>
          <CurrencySelect
            catalog={catalog}
            error={errors.settlementCurrency?.message}
            id={`${formId}-settlement`}
            label={t("activity.settlementCurrency")}
            {...register("settlementCurrency")}
          />
          <Field
            error={errors.feeAmount?.message}
            id={`${formId}-fee-amount`}
            label={t("activity.feeAmount")}
          >
            <Input
              id={`${formId}-fee-amount`}
              inputMode="decimal"
              {...register("feeAmount")}
            />
          </Field>
          <label className="flex items-center gap-2 text-sm sm:col-span-2">
            <input type="checkbox" {...register("confirmZeroUnitPrice")} />
            {t("activity.confirmZeroUnitPrice")}
          </label>
        </>
      ) : null}
      {kind === "income" ? (
        <LabeledSelect
          error={errors.incomeKind?.message}
          id={`${formId}-income-kind`}
          label={t("activity.incomeKind")}
          {...register("incomeKind")}
        >
          {INCOME_KINDS.map((value) => (
            <option key={value} value={value}>
              {t(`activity.incomeKinds.${value}`)}
            </option>
          ))}
        </LabeledSelect>
      ) : null}
      {kind === "fee" ||
      kind === "debt_payment" ||
      (kind === "transfer" && transferType === "cash") ? (
        <LabeledSelect
          error={errors.feeKind?.message}
          id={`${formId}-fee-kind`}
          label={t("activity.feeKind")}
          {...register("feeKind")}
        >
          {kind === "fee" ? null : (
            <option value="">{t("activity.noneOption")}</option>
          )}
          {FEE_KINDS.map((value) => (
            <option key={value} value={value}>
              {t(`activity.feeKinds.${value}`)}
            </option>
          ))}
        </LabeledSelect>
      ) : null}
      {kind === "income" || kind === "fee" ? (
        <LabeledSelect
          id={`${formId}-related-instrument`}
          label={t("activity.relatedInstrument")}
          {...register("instrumentId")}
        >
          <option value="">{t("activity.noneOption")}</option>
          {instruments.map((instrument) => (
            <option key={instrument.id} value={instrument.id}>
              {instrument.name}
            </option>
          ))}
        </LabeledSelect>
      ) : null}
      {kind === "debt_draw" || kind === "debt_payment" || kind === "debt_adjustment" ? (
        <>
          <AccountSelect
            accounts={
              kind === "debt_adjustment"
                ? accounts
                : accounts.filter((account) => account.primaryCategory === "liability")
            }
            error={
              kind === "debt_adjustment"
                ? errors.accountId?.message
                : errors.liabilityAccountId?.message
            }
            id={`${formId}-liability`}
            label={
              kind === "debt_adjustment"
                ? t("activity.account")
                : t("activity.liabilityAccount")
            }
            {...register(
              kind === "debt_adjustment" ? "accountId" : "liabilityAccountId",
            )}
          />
          {kind === "debt_adjustment" ? (
            <>
              <Field
                error={errors.amount?.message}
                id={`${formId}-debt-amount`}
                label={t("accounts.amount")}
              >
                <Input
                  id={`${formId}-debt-amount`}
                  inputMode="decimal"
                  {...register("amount")}
                />
              </Field>
              <CurrencySelect
                catalog={catalog}
                error={errors.currency?.message}
                id={`${formId}-debt-currency`}
                label={t("accounts.currency")}
                {...register("currency")}
              />
            </>
          ) : (
            <>
              <Field
                error={errors.principalAmount?.message}
                id={`${formId}-principal`}
                label={t("activity.principalAmount")}
              >
                <Input
                  id={`${formId}-principal`}
                  inputMode="decimal"
                  {...register("principalAmount")}
                />
              </Field>
              <CurrencySelect
                catalog={catalog}
                error={errors.principalCurrency?.message}
                id={`${formId}-principal-currency`}
                label={t("activity.principalCurrency")}
                {...register("principalCurrency")}
              />
              <AccountSelect
                accounts={accounts}
                error={errors.cashAccountId?.message}
                id={`${formId}-cash`}
                label={t("activity.cashAccount")}
                {...register("cashAccountId")}
              />
              <Field
                error={errors.cashAmount?.message}
                id={`${formId}-cash-amount`}
                label={t("activity.cashAmount")}
              >
                <Input
                  id={`${formId}-cash-amount`}
                  inputMode="decimal"
                  {...register("cashAmount")}
                />
              </Field>
              <CurrencySelect
                catalog={catalog}
                error={errors.cashCurrency?.message}
                id={`${formId}-cash-currency`}
                label={t("activity.cashCurrency")}
                {...register("cashCurrency")}
              />
            </>
          )}
        </>
      ) : null}
    </div>
  );
}

function useHoldings(accountId: string) {
  return useQuery({
    queryKey: ["holdings", accountId, true],
    enabled: accountId.length > 0,
    queryFn: () =>
      unwrapResult(commands.listHoldings({ accountId, includeArchived: true })),
  });
}

function Field({
  children,
  error,
  id,
  label,
}: {
  children: React.ReactNode;
  error?: string;
  id: string;
  label: string;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-2">
      <label className="text-sm font-medium" htmlFor={id}>
        {label}
      </label>
      {children}
      <FieldError message={translateActivityError(t, error)} />
    </div>
  );
}

function LabeledSelect({
  error,
  id,
  label,
  children,
  ...props
}: ComponentPropsWithoutRef<"select"> & {
  error?: string;
  label: string;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-2">
      <label className="text-sm font-medium" htmlFor={id}>
        {label}
      </label>
      <NativeSelect id={id} {...props}>
        {children}
      </NativeSelect>
      <FieldError message={translateActivityError(t, error)} />
    </div>
  );
}

function AccountSelect({
  accounts,
  error,
  id,
  label,
  ...props
}: ComponentPropsWithoutRef<"select"> & {
  accounts: AccountRecordDto[];
  error?: string;
  label: string;
}) {
  const { t } = useTranslation();
  return (
    <LabeledSelect error={error} id={id} label={label} {...props}>
      <option value="">{t("activity.noneOption")}</option>
      {accounts.map((account) => (
        <option key={account.id} value={account.id}>
          {account.name}
          {account.archivedAt ? ` (${t("references.archived")})` : ""}
        </option>
      ))}
    </LabeledSelect>
  );
}

function HoldingSelect({
  error,
  holdings,
  id,
  label,
  ...props
}: ComponentPropsWithoutRef<"select"> & {
  error?: string;
  holdings: HoldingRecordDto[];
  id: string;
  label: string;
}) {
  const { t } = useTranslation();
  return (
    <LabeledSelect error={error} id={id} label={label} {...props}>
      <option value="">{t("activity.noneOption")}</option>
      {holdings.map((holding) => (
        <option key={holding.id} value={holding.id}>
          {holding.instrumentName}
        </option>
      ))}
    </LabeledSelect>
  );
}

function CurrencySelect({
  catalog,
  error,
  id,
  label,
  ...props
}: ComponentPropsWithoutRef<"select"> & {
  catalog: ReferenceCatalogDto;
  error?: string;
  label: string;
}) {
  const { t } = useTranslation();
  return (
    <LabeledSelect error={error} id={id} label={label} {...props}>
      {groupReferenceOptions(catalog.currencies).map(([group, options]) => (
        <optgroup key={group} label={referenceGroupLabel(t, "currencyGroups", group)}>
          {options.map((option) => (
            <option key={option.value} value={option.value}>
              {referenceSelectOptionLabel(t, "currencies", option.value)}
            </option>
          ))}
        </optgroup>
      ))}
    </LabeledSelect>
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

function currentLocalDateTime(): { date: string; time: string } {
  const now = new Date();
  const date = [
    String(now.getFullYear()),
    String(now.getMonth() + 1).padStart(2, "0"),
    String(now.getDate()).padStart(2, "0"),
  ].join("-");
  const time = [
    String(now.getHours()).padStart(2, "0"),
    String(now.getMinutes()).padStart(2, "0"),
  ].join(":");
  return { date, time };
}
