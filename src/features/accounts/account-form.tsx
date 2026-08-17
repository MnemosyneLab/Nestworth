import { useMutation } from "@tanstack/react-query";
import { useId, useState, type ComponentPropsWithoutRef } from "react";
import { useFieldArray, useForm, type Path } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  accountSchema,
  accountToFormValues,
  categoryDefaults,
  createAccountSchema,
  emptyAccountValues,
  equalSplitPercents,
  PRIMARY_CATEGORIES,
  SECONDARY_CATEGORIES,
  toCreateAccountInput,
  toUpdateAccountInput,
  type AccountFormValues,
} from "@/features/accounts/schema";
import {
  applyServerFieldErrors,
  applyZodIssues,
  FieldError,
  translateReferenceError,
} from "@/features/references/form-helpers";
import { GhostButton } from "@/features/references/reference-page";
import {
  commands,
  type AccountRecordDto,
  type CommandError,
  type GroupRecordDto,
  type InstitutionRecordDto,
  type MemberDto,
} from "@/generated/tauri-bindings";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { cn } from "@/lib/utils";

export function translateAccountError(
  t: ReturnType<typeof useTranslation>["t"],
  message: string | undefined,
): string | undefined {
  const translated = translateReferenceError(t, message);
  if (!message || translated !== message) {
    return translated;
  }
  if (
    message === "amount" ||
    message === "percent" ||
    message === "owners" ||
    message === "ownerDuplicate" ||
    message === "ownersTotal" ||
    message === "date" ||
    message === "closedOn"
  ) {
    return t(`accounts.errors.${message}`);
  }
  return message;
}

const ACCOUNT_FIELDS: Array<Path<AccountFormValues>> = [
  "name",
  "secondaryCategory",
  "institutionId",
  "groupId",
  "note",
  "openedOn",
  "closedOn",
  "owners",
  "initialAmount",
];

export function AccountForm({
  account,
  defaultCurrency,
  groups,
  institutions,
  members,
  onCancel,
  onSaved,
}: {
  account?: AccountRecordDto;
  defaultCurrency: string;
  groups: GroupRecordDto[];
  institutions: InstitutionRecordDto[];
  members: MemberDto[];
  onCancel: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<AccountFormValues>({
    defaultValues: account
      ? accountToFormValues(account)
      : emptyAccountValues(members[0]?.id ?? ""),
  });
  const owners = useFieldArray({ control: form.control, name: "owners" });
  const ownerOptions = ownerChoices(members, account);

  const mutation = useMutation({
    mutationFn: async (values: AccountFormValues) => {
      if (account) {
        return unwrapResult(
          commands.updateAccount(toUpdateAccountInput(account.id, values)),
        );
      }
      return unwrapResult(
        commands.createAccount(toCreateAccountInput(values, defaultCurrency)),
      );
    },
    onSuccess: async () => {
      await onSaved();
    },
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      applyServerFieldErrors(form, commandError.fields, ACCOUNT_FIELDS);
    },
  });

  function applyCategory(secondary: string) {
    const defaults = categoryDefaults(secondary);
    form.setValue("includeInNetWorth", defaults.includeInNetWorth);
    form.setValue("includeInInvestment", defaults.includeInInvestment);
    form.setValue("includeInLiquidAssets", defaults.includeInLiquidAssets);
  }

  return (
    <form
      className="space-y-4 rounded-xl border border-muted bg-card px-4 py-4 shadow-sm"
      noValidate
      onSubmit={form.handleSubmit((values) => {
        const parsed = (account ? accountSchema : createAccountSchema).safeParse(
          values,
        );
        if (!parsed.success) {
          applyZodIssues(form, parsed.error.issues, ACCOUNT_FIELDS);
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
          message={translateAccountError(t, form.formState.errors.name?.message)}
        />
      </div>
      <div className="grid gap-4 sm:grid-cols-2">
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-category`}>
            {t("accounts.category")}
          </label>
          <NativeSelect
            id={`${formId}-category`}
            {...form.register("secondaryCategory", {
              onChange: (event) => applyCategory(event.target.value),
            })}
          >
            {PRIMARY_CATEGORIES.map((primary) => (
              <optgroup key={primary} label={t(`accounts.primaries.${primary}`)}>
                {SECONDARY_CATEGORIES.filter(
                  (category) => category.primary === primary,
                ).map((category) => (
                  <option key={category.secondary} value={category.secondary}>
                    {t(`accounts.categories.${category.secondary}`)}
                  </option>
                ))}
              </optgroup>
            ))}
          </NativeSelect>
        </div>
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-institution`}>
            {t("accounts.institution")}
          </label>
          <NativeSelect
            id={`${formId}-institution`}
            {...form.register("institutionId")}
          >
            <option value="">{t("accounts.none")}</option>
            {selectRecords(institutions, account?.institutionId).map((institution) => (
              <option key={institution.id} value={institution.id}>
                {institution.name}
              </option>
            ))}
          </NativeSelect>
        </div>
      </div>
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-group`}>
          {t("accounts.group")}
        </label>
        <NativeSelect id={`${formId}-group`} {...form.register("groupId")}>
          <option value="">{t("accounts.none")}</option>
          {selectRecords(groups, account?.groupId).map((group) => (
            <option key={group.id} value={group.id}>
              {group.name}
            </option>
          ))}
        </NativeSelect>
      </div>
      <fieldset className="space-y-3">
        <legend className="text-sm font-medium">{t("accounts.owners")}</legend>
        {owners.fields.map((field, index) => (
          <div className="flex flex-wrap items-end gap-2" key={field.id}>
            <div className="min-w-40 flex-1 space-y-2">
              <label className="text-sm" htmlFor={`${formId}-owner-${index}`}>
                {t("accounts.owner")}
              </label>
              <NativeSelect
                id={`${formId}-owner-${index}`}
                {...form.register(`owners.${index}.memberId`)}
              >
                {ownerOptions.map((member) => (
                  <option key={member.id} value={member.id}>
                    {member.name}
                  </option>
                ))}
              </NativeSelect>
            </div>
            <div className="w-28 space-y-2">
              <label className="text-sm" htmlFor={`${formId}-percent-${index}`}>
                {t("accounts.percent")}
              </label>
              <Input
                id={`${formId}-percent-${index}`}
                inputMode="decimal"
                type="text"
                {...form.register(`owners.${index}.percent`)}
              />
            </div>
            <GhostButton
              disabled={owners.fields.length === 1}
              onClick={() => owners.remove(index)}
              type="button"
            >
              {t("accounts.removeOwner")}
            </GhostButton>
          </div>
        ))}
        <FieldError
          message={translateAccountError(t, form.formState.errors.owners?.message)}
        />
        <div className="flex flex-wrap gap-2">
          <GhostButton
            onClick={() =>
              owners.append({
                memberId: ownerOptions[0]?.id ?? "",
                percent: "",
              })
            }
            type="button"
          >
            {t("accounts.addOwner")}
          </GhostButton>
          <GhostButton
            onClick={() => {
              const percents = equalSplitPercents(owners.fields.length);
              percents.forEach((percent, index) => {
                form.setValue(`owners.${index}.percent`, percent);
              });
            }}
            type="button"
          >
            {t("accounts.splitEqually")}
          </GhostButton>
        </div>
      </fieldset>
      {account ? null : (
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-amount`}>
            {t("accounts.amount")}
          </label>
          <Input
            aria-invalid={form.formState.errors.initialAmount ? true : undefined}
            id={`${formId}-amount`}
            inputMode="decimal"
            type="text"
            {...form.register("initialAmount")}
          />
          <FieldError
            message={translateAccountError(
              t,
              form.formState.errors.initialAmount?.message,
            )}
          />
        </div>
      )}
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" {...form.register("includeInNetWorth")} />
        {t("accounts.includeNetWorth")}
      </label>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" {...form.register("includeInInvestment")} />
        {t("accounts.includeInvestment")}
      </label>
      <label className="flex items-center gap-2 text-sm">
        <input type="checkbox" {...form.register("includeInLiquidAssets")} />
        {t("accounts.includeLiquid")}
      </label>
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-note`}>
          {t("references.note")}
        </label>
        <Textarea id={`${formId}-note`} {...form.register("note")} />
      </div>
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

function NativeSelect({ className, ...props }: ComponentPropsWithoutRef<"select">) {
  return (
    <select
      {...props}
      className={cn(
        "h-10 w-full rounded-lg border border-muted bg-card px-3 text-sm text-foreground shadow-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
    />
  );
}

function ownerChoices(members: MemberDto[], account?: AccountRecordDto): MemberDto[] {
  const choices = new Map(members.map((member) => [member.id, member]));
  for (const owner of account?.owners ?? []) {
    if (!choices.has(owner.memberId)) {
      choices.set(owner.memberId, { id: owner.memberId, name: owner.memberName });
    }
  }
  return [...choices.values()];
}

function selectRecords<T extends { id: string; archivedAt: string | null }>(
  records: T[],
  selectedId: string | null | undefined,
): T[] {
  return records.filter(
    (record) => record.archivedAt === null || record.id === selectedId,
  );
}
