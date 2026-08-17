import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import {
  Briefcase,
  Heart,
  Home,
  Shield,
  Star,
  Wallet,
  type LucideIcon,
} from "lucide-react";
import { useId, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  emptyGroupValues,
  GROUP_COLORS,
  GROUP_ICONS,
  groupSchema,
  type GroupFormValues,
} from "@/features/groups/schema";
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
  type GroupRecordDto,
} from "@/generated/tauri-bindings";
import { emptyToNull } from "@/lib/empty-to-null";
import { bootstrapQueryKey } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { cn } from "@/lib/utils";

const ICON_COMPONENTS: Record<(typeof GROUP_ICONS)[number], LucideIcon> = {
  wallet: Wallet,
  home: Home,
  shield: Shield,
  briefcase: Briefcase,
  heart: Heart,
  star: Star,
};

export function GroupsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [showArchived, setShowArchived] = useState(false);
  const [editor, setEditor] = useState<"create" | GroupRecordDto | null>(null);
  const [actionError, setActionError] = useState<CommandError | null>(null);

  const list = useQuery({
    queryKey: ["groups", showArchived],
    queryFn: () => unwrapResult(commands.listGroups({ includeArchived: showArchived })),
  });

  async function invalidate() {
    await queryClient.invalidateQueries({ queryKey: ["groups"] });
    await queryClient.invalidateQueries({ queryKey: bootstrapQueryKey });
  }

  const archive = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.archiveGroup({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const restore = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.restoreGroup({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  const items = list.data ?? [];
  const listError = list.error ? commandErrorFromUnknown(list.error) : actionError;

  return (
    <ReferencePage
      addLabel={t("groups.add")}
      empty={t("groups.empty")}
      error={listError}
      isEmpty={!editor && items.length === 0}
      loading={list.isPending}
      onAdd={() => {
        setActionError(null);
        setEditor("create");
      }}
      onShowArchivedChange={setShowArchived}
      showArchived={showArchived}
      title={t("groups.title")}
    >
      <div className="space-y-3">
        {editor === "create" ? (
          <GroupEditor
            onCancel={() => setEditor(null)}
            onSaved={async () => {
              setEditor(null);
              await invalidate();
            }}
          />
        ) : null}
        {items.map((group) =>
          editor !== "create" && editor?.id === group.id ? (
            <GroupEditor
              group={group}
              key={group.id}
              onCancel={() => setEditor(null)}
              onSaved={async () => {
                setEditor(null);
                await invalidate();
              }}
            />
          ) : (
            <RecordCard
              archived={Boolean(group.archivedAt)}
              details={
                group.description ? (
                  <p className="mt-1 text-sm text-muted-foreground">
                    {group.description}
                  </p>
                ) : null
              }
              key={group.id}
              name={group.name}
            >
              <GroupMark color={group.color} iconKey={group.iconKey} />
              <GhostButton
                onClick={() => {
                  setActionError(null);
                  setEditor(group);
                }}
                type="button"
              >
                {t("references.edit")}
              </GhostButton>
              {group.archivedAt ? (
                <GhostButton
                  aria-label={t("groups.restoreName", { name: group.name })}
                  disabled={restore.isPending}
                  onClick={() => restore.mutate(group.id)}
                  type="button"
                >
                  {t("references.restore")}
                </GhostButton>
              ) : (
                <GhostButton
                  aria-label={t("groups.archiveName", { name: group.name })}
                  disabled={archive.isPending}
                  onClick={() => archive.mutate(group.id)}
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

function GroupMark({
  color,
  iconKey,
}: {
  color: string | null;
  iconKey: string | null;
}) {
  const Icon =
    iconKey && iconKey in ICON_COMPONENTS
      ? ICON_COMPONENTS[iconKey as keyof typeof ICON_COMPONENTS]
      : null;
  return (
    <span
      aria-hidden="true"
      className="mr-auto flex h-9 w-9 items-center justify-center rounded-lg"
      style={{
        backgroundColor: color ?? "var(--muted)",
        color: color ? "#fff" : undefined,
      }}
    >
      {Icon ? <Icon size={16} /> : null}
    </span>
  );
}

function GroupEditor({
  group,
  onCancel,
  onSaved,
}: {
  group?: GroupRecordDto;
  onCancel: () => void;
  onSaved: () => Promise<void>;
}) {
  const { t } = useTranslation();
  const formId = useId();
  const [serverError, setServerError] = useState<CommandError | null>(null);
  const form = useForm<GroupFormValues>({
    defaultValues: group
      ? {
          name: group.name,
          iconKey: group.iconKey ?? "",
          color: group.color ?? "",
          description: group.description ?? "",
        }
      : emptyGroupValues,
  });
  const iconKey = form.watch("iconKey");
  const color = form.watch("color");

  const mutation = useMutation({
    mutationFn: async (values: GroupFormValues) => {
      const payload = {
        name: values.name.trim(),
        iconKey: emptyToNull(values.iconKey),
        color: emptyToNull(values.color),
        description: emptyToNull(values.description),
      };
      if (group) {
        return unwrapResult(commands.updateGroup({ id: group.id, ...payload }));
      }
      return unwrapResult(commands.createGroup(payload));
    },
    onSuccess: async () => {
      await onSaved();
    },
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      applyServerFieldErrors(form, commandError.fields, [
        "name",
        "iconKey",
        "color",
        "description",
      ]);
    },
  });

  return (
    <form
      className="space-y-4 rounded-xl border border-muted bg-card px-4 py-4 shadow-sm"
      noValidate
      onSubmit={form.handleSubmit((values) => {
        const parsed = groupSchema.safeParse(values);
        if (!parsed.success) {
          applyZodIssues(form, parsed.error.issues, [
            "name",
            "iconKey",
            "color",
            "description",
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
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium">{t("groups.icon")}</legend>
        <div className="flex flex-wrap gap-2">
          {GROUP_ICONS.map((key) => {
            const Icon = ICON_COMPONENTS[key];
            return (
              <button
                aria-label={t(`groups.icons.${key}`)}
                aria-pressed={iconKey === key}
                className={cn(
                  "flex h-10 w-10 items-center justify-center rounded-lg border",
                  iconKey === key ? "border-ring bg-muted" : "border-muted bg-card",
                )}
                key={key}
                onClick={() => form.setValue("iconKey", iconKey === key ? "" : key)}
                type="button"
              >
                <Icon size={16} />
              </button>
            );
          })}
        </div>
      </fieldset>
      <fieldset className="space-y-2">
        <legend className="text-sm font-medium">{t("groups.color")}</legend>
        <div className="flex flex-wrap gap-2">
          {GROUP_COLORS.map((value) => (
            <button
              aria-label={value}
              aria-pressed={color.toUpperCase() === value}
              className={cn(
                "h-8 w-8 rounded-full border-2",
                color.toUpperCase() === value
                  ? "border-foreground"
                  : "border-transparent",
              )}
              key={value}
              onClick={() =>
                form.setValue("color", color.toUpperCase() === value ? "" : value)
              }
              style={{ backgroundColor: value }}
              type="button"
            />
          ))}
        </div>
        <FieldError
          message={translateReferenceError(t, form.formState.errors.color?.message)}
        />
      </fieldset>
      <div className="space-y-2">
        <label className="text-sm font-medium" htmlFor={`${formId}-description`}>
          {t("groups.description")}
        </label>
        <Textarea id={`${formId}-description`} {...form.register("description")} />
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
