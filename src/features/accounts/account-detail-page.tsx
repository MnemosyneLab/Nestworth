import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useId, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { AppShell } from "@/components/app-shell";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { AccountForm, translateAccountError } from "@/features/accounts/account-form";
import {
  formatMoney,
  updateValueSchema,
  type UpdateValueFormValues,
} from "@/features/accounts/schema";
import {
  applyServerFieldErrors,
  applyZodIssues,
  FieldError,
} from "@/features/references/form-helpers";
import { GhostButton } from "@/features/references/reference-page";
import { commands, type CommandError } from "@/generated/tauri-bindings";
import { bootstrapQueryKey, useBootstrapQuery } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";

export function AccountDetailPage({ accountId }: { accountId: string }) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const bootstrap = useBootstrapQuery();
  const household =
    bootstrap.data?.status === "ready" ? bootstrap.data.household : null;
  const members = bootstrap.data?.status === "ready" ? bootstrap.data.members : [];
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
    await queryClient.invalidateQueries({ queryKey: ["account", accountId] });
    await queryClient.invalidateQueries({ queryKey: ["accounts"] });
    await queryClient.invalidateQueries({ queryKey: bootstrapQueryKey });
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
                  {account.latestValue
                    ? formatMoney(
                        account.latestValue.amount,
                        account.latestValue.currency,
                      )
                    : t("accounts.noValue")}
                </p>
                <p className="mt-1 text-sm text-muted-foreground">
                  {t(`accounts.categories.${account.secondaryCategory}`)}
                </p>
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
            <UpdateValueForm
              accountId={account.id}
              onError={setActionError}
              onSaved={invalidate}
            />
            {editing ? (
              <AccountForm
                account={account}
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
      className="space-y-4 rounded-xl border border-muted bg-card px-4 py-4 shadow-sm"
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
