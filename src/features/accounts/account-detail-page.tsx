import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useId, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { AppShell } from "@/components/app-shell";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { AccountForm, translateAccountError } from "@/features/accounts/account-form";
import { AccountMark } from "@/features/accounts/account-mark";
import {
  cashSchema,
  holdingSchema,
  updateValueSchema,
  type CashFormValues,
  type HoldingFormValues,
  type UpdateValueFormValues,
} from "@/features/accounts/schema";
import { UnvaluedList, ValuationSummary } from "@/features/valuation/status";
import {
  applyServerFieldErrors,
  applyZodIssues,
  FieldError,
} from "@/features/references/form-helpers";
import { GhostButton } from "@/features/references/reference-page";
import {
  commands,
  type CommandError,
  type ReferenceCatalogDto,
} from "@/generated/tauri-bindings";
import {
  formatReferenceMoney,
  groupReferenceOptions,
  hasReferenceValue,
  referenceCurrencyCodeLabel,
  referenceGroupLabel,
  referenceSelectOptionLabel,
  referenceCatalogFromBootstrap,
} from "@/lib/reference-catalog";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { invalidateValuation } from "@/lib/tauri/invalidate";
import { emptyToNull } from "@/lib/empty-to-null";

export function AccountDetailPage({ accountId }: { accountId: string }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const bootstrap = useBootstrapQuery();
  const household =
    bootstrap.data?.status === "ready" ? bootstrap.data.household : null;
  const members = bootstrap.data?.status === "ready" ? bootstrap.data.members : [];
  const catalog = referenceCatalogFromBootstrap(bootstrap.data);
  const [editing, setEditing] = useState(false);
  const [actionError, setActionError] = useState<CommandError | null>(null);

  const detail = useQuery({
    queryKey: ["account", accountId],
    queryFn: () => unwrapResult(commands.getAccount({ id: accountId })),
  });
  const institutions = useQuery({
    queryKey: ["institutions", true],
    queryFn: () => unwrapResult(commands.listInstitutions({ includeArchived: true })),
  });
  const groups = useQuery({
    queryKey: ["groups", true],
    queryFn: () => unwrapResult(commands.listGroups({ includeArchived: true })),
  });

  async function invalidate() {
    await invalidateValuation(queryClient, accountId);
  }

  const archive = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.archiveAccount({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const restore = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.restoreAccount({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  const account = detail.data;
  const error = detail.error ? commandErrorFromUnknown(detail.error) : actionError;

  return (
    <AppShell>
      <main className="mx-auto max-w-3xl px-8 py-10">
        <Link
          className="text-sm text-muted-foreground hover:text-foreground"
          search={(prev) => prev}
          to="/accounts"
        >
          {t("accounts.back")}
        </Link>
        {detail.isPending ? (
          <p className="mt-6" role="status">
            {t("references.loading")}
          </p>
        ) : null}
        {error ? (
          <p className="mt-6 text-sm text-destructive" role="alert">
            {formatCommandError(t, error)}
          </p>
        ) : null}
        {account && household ? (
          <div className="mt-6 space-y-6">
            <div className="flex flex-wrap items-start justify-between gap-3">
              <div className="flex min-w-0 items-start gap-3">
                <AccountMark account={account} institutions={institutions.data ?? []} />
                <div>
                  <h1 className="text-3xl font-semibold tracking-tight">
                    {account.name}
                  </h1>
                  {account.archivedAt ? (
                    <p className="mt-1 text-xs uppercase tracking-wide text-muted-foreground">
                      {t("references.archived")}
                    </p>
                  ) : null}
                  <p className="mt-2 text-lg text-muted-foreground">
                    {account.valuation.base
                      ? formatReferenceMoney(
                          t,
                          catalog,
                          account.valuation.base.amount,
                          account.valuation.base.currency,
                        )
                      : t("accounts.noValue")}
                  </p>
                  <ValuationSummary
                    base={account.valuation.base}
                    catalog={catalog}
                    complete={account.valuation.complete}
                    freshness={account.valuation.freshness}
                    native={account.valuation.native}
                  />
                  <p className="mt-1 text-sm text-muted-foreground">
                    {t(`accounts.categories.${account.secondaryCategory}`)}
                  </p>
                </div>
              </div>
              <div className="flex flex-wrap gap-2">
                <GhostButton
                  onClick={() => {
                    setActionError(null);
                    setEditing((value) => !value);
                  }}
                  type="button"
                >
                  {editing ? t("references.cancel") : t("references.edit")}
                </GhostButton>
                {account.archivedAt ? (
                  <GhostButton
                    aria-label={t("accounts.restoreName", { name: account.name })}
                    disabled={restore.isPending}
                    onClick={() => restore.mutate(account.id)}
                    type="button"
                  >
                    {t("references.restore")}
                  </GhostButton>
                ) : (
                  <GhostButton
                    aria-label={t("accounts.archiveName", { name: account.name })}
                    disabled={archive.isPending}
                    onClick={() => archive.mutate(account.id)}
                    type="button"
                  >
                    {t("references.archive")}
                  </GhostButton>
                )}
              </div>
            </div>
            <UnvaluedList items={account.valuation.unvaluedItems} />
            {account.trackingMode === "holdings" ? (
              <>
                <HoldingsPanel
                  accountId={account.id}
                  catalog={catalog}
                  onError={setActionError}
                />
                <CashPanel
                  accountId={account.id}
                  catalog={catalog}
                  defaultCurrency={account.defaultCurrency}
                  onError={setActionError}
                />
              </>
            ) : (
              <UpdateValueForm
                accountId={account.id}
                onError={setActionError}
                onSaved={invalidate}
              />
            )}
            {editing ? (
              <AccountForm
                account={account}
                catalog={catalog}
                defaultCurrency={household.baseCurrency}
                groups={groups.data ?? []}
                institutions={institutions.data ?? []}
                members={members}
                onCancel={() => setEditing(false)}
                onSaved={async () => {
                  setEditing(false);
                  await invalidate();
                }}
              />
            ) : null}
          </div>
        ) : null}
      </main>
    </AppShell>
  );
}

function UpdateValueForm({
  accountId,
  onError,
  onSaved,
}: {
  accountId: string;
  onError: (error: CommandError | null) => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<UpdateValueFormValues>({ defaultValues: { amount: "" } });
  const mutation = useMutation({
    mutationFn: async (values: UpdateValueFormValues) =>
      unwrapResult(
        commands.updateAccountValue({ id: accountId, amount: values.amount.trim() }),
      ),
    onSuccess: async () => {
      form.reset({ amount: "" });
      await onSaved();
    },
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      onError(commandError);
      applyServerFieldErrors(form, commandError.fields, ["amount"]);
    },
  });

  return (
    <form
      className="space-y-4 rounded-xl border border-border bg-card px-4 py-4 shadow-sm"
      noValidate
      onSubmit={form.handleSubmit((values) => {
        const parsed = updateValueSchema.safeParse(values);
        if (!parsed.success) {
          applyZodIssues(form, parsed.error.issues, ["amount"]);
          return;
        }
        setServerError(null);
        onError(null);
        mutation.mutate(parsed.data);
      })}
    >
      <h2 className="text-lg font-medium">{t("accounts.updateValue")}</h2>
      {serverError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, serverError)}
        </p>
      ) : null}
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-amount`}>
          {t("accounts.amount")}
        </label>
        <Input
          aria-invalid={form.formState.errors.amount ? true : undefined}
          id={`${formId}-amount`}
          inputMode="decimal"
          type="text"
          {...form.register("amount")}
        />
        <FieldError
          message={translateAccountError(t, form.formState.errors.amount?.message)}
        />
      </div>
      <Button disabled={mutation.isPending} type="submit">
        {mutation.isPending ? t("references.saving") : t("references.save")}
      </Button>
    </form>
  );
}

function HoldingsPanel({
  accountId,
  catalog,
  onError,
}: {
  accountId: string;
  catalog: ReferenceCatalogDto;
  onError: (error: CommandError | null) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [adding, setAdding] = useState(false);
  const holdings = useQuery({
    queryKey: ["holdings", accountId, false],
    queryFn: () =>
      unwrapResult(commands.listHoldings({ accountId, includeArchived: false })),
  });
  const instruments = useQuery({
    queryKey: ["instruments", false],
    queryFn: () => unwrapResult(commands.listInstruments({ includeArchived: false })),
  });

  async function invalidate() {
    await invalidateValuation(queryClient, accountId);
  }

  const archive = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.archiveHolding({ id })),
    onSuccess: invalidate,
    onError: (error) => onError(commandErrorFromUnknown(error)),
  });

  return (
    <section className="space-y-3 rounded-xl border border-border bg-card px-4 py-4 shadow-sm">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-lg font-medium">{t("accounts.holdings")}</h2>
        <GhostButton onClick={() => setAdding((value) => !value)} type="button">
          {adding ? t("references.cancel") : t("accounts.addHolding")}
        </GhostButton>
      </div>
      {adding ? (
        <HoldingForm
          accountId={accountId}
          instrumentIds={(instruments.data ?? []).map((item) => ({
            id: item.id,
            name: item.name,
          }))}
          onError={onError}
          onSaved={async () => {
            setAdding(false);
            await invalidate();
          }}
        />
      ) : null}
      {holdings.isPending ? <p role="status">{t("references.loading")}</p> : null}
      {(holdings.data ?? []).length === 0 && !adding ? (
        <p className="text-sm text-muted-foreground">{t("accounts.noHoldings")}</p>
      ) : null}
      {(holdings.data ?? []).map((holding) => (
        <article
          className="flex flex-wrap items-center justify-between gap-2"
          key={holding.id}
        >
          <div>
            <p className="font-medium">{holding.instrumentName}</p>
            <p className="text-sm text-muted-foreground">
              {holding.quantity} ·{" "}
              {referenceCurrencyCodeLabel(t, catalog, holding.quoteCurrency)}
            </p>
          </div>
          <GhostButton
            aria-label={t("accounts.archiveHolding", { name: holding.instrumentName })}
            disabled={archive.isPending}
            onClick={() => archive.mutate(holding.id)}
            type="button"
          >
            {t("references.archive")}
          </GhostButton>
        </article>
      ))}
    </section>
  );
}

function HoldingForm({
  accountId,
  instrumentIds,
  onError,
  onSaved,
}: {
  accountId: string;
  instrumentIds: Array<{ id: string; name: string }>;
  onError: (error: CommandError | null) => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<HoldingFormValues>({
    defaultValues: {
      instrumentId: instrumentIds[0]?.id ?? "",
      quantity: "",
      note: "",
    },
  });
  const mutation = useMutation({
    mutationFn: async (values: HoldingFormValues) =>
      unwrapResult(
        commands.createHolding({
          accountId,
          instrumentId: values.instrumentId,
          quantity: values.quantity.trim(),
          note: emptyToNull(values.note),
        }),
      ),
    onSuccess: onSaved,
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      onError(commandError);
    },
  });

  return (
    <form
      className="space-y-3"
      noValidate
      onSubmit={form.handleSubmit((values) => {
        const parsed = holdingSchema.safeParse(values);
        if (!parsed.success) {
          applyZodIssues(form, parsed.error.issues, [
            "instrumentId",
            "quantity",
            "note",
          ]);
          return;
        }
        setServerError(null);
        onError(null);
        mutation.mutate(parsed.data);
      })}
    >
      {serverError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, serverError)}
        </p>
      ) : null}
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-instrument`}>
          {t("accounts.instrument")}
        </label>
        <select
          className="h-10 w-full rounded-lg border border-border bg-card px-3 text-sm"
          id={`${formId}-instrument`}
          {...form.register("instrumentId")}
        >
          {instrumentIds.map((instrument) => (
            <option key={instrument.id} value={instrument.id}>
              {instrument.name}
            </option>
          ))}
        </select>
      </div>
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-quantity`}>
          {t("accounts.quantity")}
        </label>
        <Input
          id={`${formId}-quantity`}
          inputMode="decimal"
          type="text"
          {...form.register("quantity")}
        />
        <FieldError
          message={translateAccountError(t, form.formState.errors.quantity?.message)}
        />
      </div>
      <Button disabled={mutation.isPending} type="submit">
        {mutation.isPending ? t("references.saving") : t("references.save")}
      </Button>
    </form>
  );
}

function CashPanel({
  accountId,
  catalog,
  defaultCurrency,
  onError,
}: {
  accountId: string;
  catalog: ReferenceCatalogDto;
  defaultCurrency: string;
  onError: (error: CommandError | null) => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [adding, setAdding] = useState(false);
  const cash = useQuery({
    queryKey: ["cash", accountId],
    queryFn: () => unwrapResult(commands.listAccountCash({ accountId })),
  });

  async function invalidate() {
    await invalidateValuation(queryClient, accountId);
  }

  return (
    <section className="space-y-3 rounded-xl border border-border bg-card px-4 py-4 shadow-sm">
      <div className="flex items-center justify-between gap-2">
        <h2 className="text-lg font-medium">{t("accounts.cash")}</h2>
        <GhostButton onClick={() => setAdding((value) => !value)} type="button">
          {adding ? t("references.cancel") : t("accounts.addCash")}
        </GhostButton>
      </div>
      {adding ? (
        <CashForm
          accountId={accountId}
          catalog={catalog}
          defaultCurrency={defaultCurrency}
          onError={onError}
          onSaved={async () => {
            setAdding(false);
            await invalidate();
          }}
        />
      ) : null}
      {(cash.data ?? []).length === 0 && !adding ? (
        <p className="text-sm text-muted-foreground">{t("accounts.noCash")}</p>
      ) : null}
      {(cash.data ?? []).map((item) => (
        <p className="text-sm" key={item.id}>
          {formatReferenceMoney(t, catalog, item.amount, item.currency)}
        </p>
      ))}
    </section>
  );
}

function CashForm({
  accountId,
  catalog,
  defaultCurrency,
  onError,
  onSaved,
}: {
  accountId: string;
  catalog: ReferenceCatalogDto;
  defaultCurrency: string;
  onError: (error: CommandError | null) => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const formId = useId();
  const initialCurrency = hasReferenceValue(catalog.currencies, defaultCurrency)
    ? defaultCurrency
    : (catalog.currencies[0]?.value ?? "");
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<CashFormValues>({
    defaultValues: { amount: "", currency: initialCurrency },
  });
  const mutation = useMutation({
    mutationFn: async (values: CashFormValues) =>
      unwrapResult(
        commands.appendAccountCash({
          accountId,
          amount: values.amount.trim(),
          currency: values.currency.trim(),
        }),
      ),
    onSuccess: onSaved,
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      onError(commandError);
    },
  });

  return (
    <form
      className="space-y-3"
      noValidate
      onSubmit={form.handleSubmit((values) => {
        const parsed = cashSchema.safeParse({
          ...values,
          currency: values.currency.trim().toUpperCase(),
        });
        if (!parsed.success) {
          applyZodIssues(form, parsed.error.issues, ["amount", "currency"]);
          return;
        }
        setServerError(null);
        onError(null);
        mutation.mutate(parsed.data);
      })}
    >
      {serverError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, serverError)}
        </p>
      ) : null}
      <div className="grid gap-3 sm:grid-cols-2">
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-amount`}>
            {t("accounts.amount")}
          </label>
          <Input
            id={`${formId}-amount`}
            inputMode="decimal"
            type="text"
            {...form.register("amount")}
          />
          <FieldError
            message={translateAccountError(t, form.formState.errors.amount?.message)}
          />
        </div>
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-currency`}>
            {t("accounts.currency")}
          </label>
          <select
            id={`${formId}-currency`}
            className="h-10 w-full rounded-lg border border-border bg-card px-3 text-sm text-foreground shadow-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring"
            {...form.register("currency")}
          >
            {groupReferenceOptions(catalog.currencies).map(([group, options]) => (
              <optgroup
                key={group}
                label={referenceGroupLabel(t, "currencyGroups", group)}
              >
                {options.map((option) => (
                  <option key={option.value} value={option.value}>
                    {referenceSelectOptionLabel(t, "currencies", option.value)}
                  </option>
                ))}
              </optgroup>
            ))}
          </select>
          <FieldError
            message={translateAccountError(t, form.formState.errors.currency?.message)}
          />
        </div>
      </div>
      <Button disabled={mutation.isPending} type="submit">
        {mutation.isPending ? t("references.saving") : t("references.save")}
      </Button>
    </form>
  );
}
