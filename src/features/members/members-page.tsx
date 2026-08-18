import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useId, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import { ImagePicker } from "@/features/media/image-picker";
import { MediaImage } from "@/features/media/media-image";
import {
  emptyMemberValues,
  memberSchema,
  type MemberFormValues,
} from "@/features/members/schema";
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
  type MemberRecordDto,
} from "@/generated/tauri-bindings";
import { emptyToNull } from "@/lib/empty-to-null";
import { bootstrapQueryKey } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";

export const membersQueryKey = (includeArchived: boolean) =>
  ["members", includeArchived] as const;

export function MembersPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [showArchived, setShowArchived] = useState(false);
  const [editor, setEditor] = useState<"create" | MemberRecordDto | null>(null);
  const [actionError, setActionError] = useState<CommandError | null>(null);

  const list = useQuery({
    queryKey: membersQueryKey(showArchived),
    queryFn: () =>
      unwrapResult(commands.listMembers({ includeArchived: showArchived })),
  });

  async function invalidate() {
    await queryClient.invalidateQueries({ queryKey: ["members"] });
    await queryClient.invalidateQueries({ queryKey: bootstrapQueryKey });
  }

  const archive = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.archiveMember({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const restore = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.restoreMember({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  const items = list.data ?? [];
  const listError = list.error ? commandErrorFromUnknown(list.error) : actionError;

  return (
    <ReferencePage
      addLabel={t("members.add")}
      empty={t("members.empty")}
      error={listError}
      isEmpty={!editor && items.length === 0}
      loading={list.isPending}
      onAdd={() => {
        setActionError(null);
        setEditor("create");
      }}
      onShowArchivedChange={setShowArchived}
      showArchived={showArchived}
      title={t("members.title")}
    >
      <div className="space-y-3">
        {editor === "create" ? (
          <MemberEditor
            onCancel={() => setEditor(null)}
            onSaved={async () => {
              setEditor(null);
              await invalidate();
            }}
          />
        ) : null}
        {items.map((member) =>
          editor !== "create" && editor?.id === member.id ? (
            <MemberEditor
              key={member.id}
              member={member}
              onCancel={() => setEditor(null)}
              onSaved={async () => {
                setEditor(null);
                await invalidate();
              }}
            />
          ) : (
            <RecordCard
              archived={Boolean(member.archivedAt)}
              details={
                member.note ? (
                  <p className="mt-1 text-sm text-muted-foreground">{member.note}</p>
                ) : null
              }
              key={member.id}
              leading={<MediaImage alt="" assetId={member.avatarAssetId} />}
              name={member.name}
            >
              <GhostButton
                onClick={() => {
                  setActionError(null);
                  setEditor(member);
                }}
                type="button"
              >
                {t("references.edit")}
              </GhostButton>
              {member.archivedAt ? (
                <GhostButton
                  aria-label={t("members.restoreName", { name: member.name })}
                  disabled={restore.isPending}
                  onClick={() => restore.mutate(member.id)}
                  type="button"
                >
                  {t("references.restore")}
                </GhostButton>
              ) : (
                <GhostButton
                  aria-label={t("members.archiveName", { name: member.name })}
                  disabled={archive.isPending}
                  onClick={() => archive.mutate(member.id)}
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

function MemberEditor({
  member,
  onCancel,
  onSaved,
}: {
  member?: MemberRecordDto;
  onCancel: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<MemberFormValues>({
    defaultValues: member
      ? { name: member.name, note: member.note ?? "" }
      : emptyMemberValues,
  });

  const mutation = useMutation({
    mutationFn: async (values: MemberFormValues) => {
      const note = emptyToNull(values.note);
      if (member) {
        return unwrapResult(
          commands.updateMember({ id: member.id, name: values.name.trim(), note }),
        );
      }
      return unwrapResult(commands.createMember({ name: values.name.trim(), note }));
    },
    onSuccess: async () => {
      await onSaved();
    },
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      applyServerFieldErrors(form, commandError.fields, ["name", "note"]);
    },
  });

  return (
    <form
      className="space-y-4 rounded-xl border border-muted bg-card px-4 py-4 shadow-sm"
      noValidate
      onSubmit={form.handleSubmit((values) => {
        const parsed = memberSchema.safeParse(values);
        if (!parsed.success) {
          applyZodIssues(form, parsed.error.issues, ["name", "note"]);
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
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-note`}>
          {t("references.note")}
        </label>
        <Textarea id={`${formId}-note`} {...form.register("note")} />
        <FieldError
          message={translateReferenceError(t, form.formState.errors.note?.message)}
        />
      </div>
      {member ? (
        <ImagePicker
          assetId={member.avatarAssetId}
          entityId={member.id}
          kind="memberAvatar"
          onSaved={onSaved}
        />
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

export default MembersPage;
