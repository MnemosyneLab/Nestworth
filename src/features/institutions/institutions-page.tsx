import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  emptyInstitutionValues,
  institutionSchema,
  type InstitutionFormValues,
} from "@/features/institutions/schema";
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
import {
  commands,
  type CommandError,
  type InstitutionRecordDto,
} from "@/generated/tauri-bindings";
import { emptyToNull } from "@/lib/empty-to-null";
import { bootstrapQueryKey } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";

export function InstitutionsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [showArchived, setShowArchived] = useState(false);
  const [editor, setEditor] = useState<"create" | InstitutionRecordDto | null>(null);
  const [actionError, setActionError] = useState<CommandError | null>(null);

  const list = useQuery({
    queryKey: ["institutions", showArchived],
    queryFn: () =>
      unwrapResult(commands.listInstitutions({ includeArchived: showArchived })),
  });

  async function invalidate() {
    await queryClient.invalidateQueries({ queryKey: ["institutions"] });
    await queryClient.invalidateQueries({ queryKey: bootstrapQueryKey });
  }

  const archive = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.archiveInstitution({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const restore = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.restoreInstitution({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  const items = list.data ?? [];
  const listError = list.error ? commandErrorFromUnknown(list.error) : actionError;

  return (
    <ReferencePage
      addLabel={t("institutions.add")}
      empty={t("institutions.empty")}
      error={listError}
      isEmpty={!editor && items.length === 0}
      loading={list.isPending}
      onAdd={() => {
        setActionError(null);
        setEditor("create");
      }}
      onShowArchivedChange={setShowArchived}
      showArchived={showArchived}
      title={t("institutions.title")}
    >
      <div className="space-y-3">
        {editor === "create" ? (
          <InstitutionEditor
            onCancel={() => setEditor(null)}
            onSaved={async () => {
              setEditor(null);
              await invalidate();
            }}
          />
        ) : null}
        {items.map((institution) =>
          editor !== "create" && editor?.id === institution.id ? (
            <InstitutionEditor
              institution={institution}
              key={institution.id}
              onCancel={() => setEditor(null)}
              onSaved={async () => {
                setEditor(null);
                await invalidate();
              }}
            />
          ) : (
            <RecordCard
              archived={Boolean(institution.archivedAt)}
              details={
                <p className="mt-1 text-sm text-muted-foreground">
                  {[institution.institutionType, institution.countryCode]
                    .filter(Boolean)
                    .join(" · ")}
                </p>
              }
              key={institution.id}
              name={institution.name}
            >
              <GhostButton
                onClick={() => {
                  setActionError(null);
                  setEditor(institution);
                }}
                type="button"
              >
                {t("references.edit")}
              </GhostButton>
              {institution.archivedAt ? (
                <GhostButton
                  aria-label={t("institutions.restoreName", {
                    name: institution.name,
                  })}
                  disabled={restore.isPending}
                  onClick={() => restore.mutate(institution.id)}
                  type="button"
                >
                  {t("references.restore")}
                </GhostButton>
              ) : (
                <GhostButton
                  aria-label={t("institutions.archiveName", {
                    name: institution.name,
                  })}
                  disabled={archive.isPending}
                  onClick={() => archive.mutate(institution.id)}
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

function InstitutionEditor({
  institution,
  onCancel,
  onSaved,
}: {
  institution?: InstitutionRecordDto;
  onCancel: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<InstitutionFormValues>({
    defaultValues: institution
      ? {
          name: institution.name,
          institutionType: institution.institutionType ?? "",
          countryCode: institution.countryCode ?? "",
          website: institution.website ?? "",
          note: institution.note ?? "",
        }
      : emptyInstitutionValues,
  });

  const mutation = useMutation({
    mutationFn: async (values: InstitutionFormValues) => {
      const payload = {
        name: values.name.trim(),
        institutionType: emptyToNull(values.institutionType),
        countryCode: emptyToNull(values.countryCode),
        website: emptyToNull(values.website),
        note: emptyToNull(values.note),
      };
      if (institution) {
        return unwrapResult(
          commands.updateInstitution({ id: institution.id, ...payload }),
        );
      }
      return unwrapResult(commands.createInstitution(payload));
    },
    onSuccess: async () => {
      await onSaved();
    },
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      applyServerFieldErrors(form, commandError.fields, [
        "name",
        "institutionType",
        "countryCode",
        "website",
        "note",
      ]);
    },
  });

  return (
    <form
      className="space-y-4 rounded-xl border border-muted bg-card px-4 py-4 shadow-sm"
      noValidate
      onSubmit={form.handleSubmit((values) => {
        const parsed = institutionSchema.safeParse({
          ...values,
          countryCode: values.countryCode.trim().toUpperCase(),
        });
        if (!parsed.success) {
          applyZodIssues(form, parsed.error.issues, [
            "name",
            "institutionType",
            "countryCode",
            "website",
            "note",
          ]);
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
          <label className="text-sm font-medium" htmlFor={`${formId}-type`}>
            {t("institutions.type")}
          </label>
          <Input id={`${formId}-type`} {...form.register("institutionType")} />
        </div>
        <div className="space-y-2">
          <label className="text-sm font-medium" htmlFor={`${formId}-country`}>
            {t("institutions.country")}
          </label>
          <Input
            aria-invalid={form.formState.errors.countryCode ? true : undefined}
            autoCapitalize="characters"
            id={`${formId}-country`}
            maxLength={2}
            {...form.register("countryCode")}
          />
          <FieldError
            message={translateReferenceError(
              t,
              form.formState.errors.countryCode?.message,
            )}
          />
        </div>
      </div>
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-website`}>
          {t("institutions.website")}
        </label>
        <Input id={`${formId}-website`} {...form.register("website")} />
      </div>
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
