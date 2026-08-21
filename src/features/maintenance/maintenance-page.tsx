import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState } from "react";
import { useTranslation } from "react-i18next";
import { AppShell } from "@/components/app-shell";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  commands,
  type AccountRecordDto,
  type CommandError,
  type FreshnessPolicyDto,
  type MaintenanceItemDto,
  type PendingActivityDto,
  type PendingActivityPayloadInput,
  type RecurringActivityRuleDto,
} from "@/generated/tauri-bindings";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";

const PAYLOAD_KINDS = [
  "deposit",
  "withdrawal",
  "transfer",
  "position_transfer",
  "buy",
  "sell",
  "income",
  "fee",
  "debt_draw",
  "debt_payment",
] as const;
type PayloadKind = (typeof PAYLOAD_KINDS)[number];
type FieldValues = Record<string, string>;

const EMPTY_VALUES: FieldValues = {
  accountId: "",
  component: "account_value",
  amount: "",
  currency: "CNY",
  sourceAccountId: "",
  sourceComponent: "account_value",
  sourceAmount: "",
  sourceCurrency: "CNY",
  destinationAccountId: "",
  destinationComponent: "account_value",
  destinationAmount: "",
  destinationCurrency: "CNY",
  feeAmount: "",
  feeKind: "other",
  sourceHoldingId: "",
  destinationHoldingId: "",
  quantity: "",
  holdingId: "",
  instrumentId: "",
  unitPrice: "",
  grossAmount: "",
  settlementCurrency: "CNY",
  incomeKind: "salary",
  instrumentIdOptional: "",
  liabilityAccountId: "",
  principalAmount: "",
  principalCurrency: "CNY",
  cashAccountId: "",
  cashComponent: "account_value",
  cashAmount: "",
  cashCurrency: "CNY",
  fxRate: "",
};

export function MaintenancePage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const bootstrap = useBootstrapQuery();
  const catalog =
    bootstrap.data?.status === "ready" ? bootstrap.data.referenceCatalog : null;
  const [actionError, setActionError] = useState<CommandError | null>(null);
  const [showPendingForm, setShowPendingForm] = useState(false);
  const [showRuleForm, setShowRuleForm] = useState(false);
  const [generationMessage, setGenerationMessage] = useState<string | null>(null);
  const [editingPending, setEditingPending] = useState<PendingActivityDto | null>(null);
  const [pendingPreview, setPendingPreview] = useState<string | null>(null);
  const [confirmPost, setConfirmPost] = useState<string | null>(null);
  const [confirmSkip, setConfirmSkip] = useState<string | null>(null);

  const accounts = useQuery({
    queryKey: ["accounts", false],
    queryFn: () => unwrapResult(commands.listAccounts({ includeArchived: false })),
  });
  const maintenance = useQuery({
    queryKey: ["maintenance"],
    queryFn: () => unwrapResult(commands.listMaintenanceItems()),
  });
  const pending = useQuery({
    queryKey: ["pending", "open"],
    queryFn: () =>
      unwrapResult(
        commands.listPendingActivities({ cursor: null, limit: 100, status: "open" }),
      ),
  });
  const rules = useQuery({
    queryKey: ["recurring-rules"],
    queryFn: () =>
      unwrapResult(commands.listRecurringActivityRules({ includeArchived: true })),
  });
  const policies = useQuery({
    queryKey: ["freshness-policies"],
    queryFn: () =>
      unwrapResult(commands.listFreshnessPolicies({ includeArchived: true })),
  });

  const refresh = async () => {
    await Promise.all([
      maintenance.refetch(),
      pending.refetch(),
      rules.refetch(),
      policies.refetch(),
    ]);
  };
  const generate = useMutation({
    mutationFn: () => unwrapResult(commands.generateDuePendingActivities()),
    onSuccess: async (result) => {
      setActionError(null);
      setGenerationMessage(
        result.hasMore
          ? t("maintenance.generationMore", { count: result.generatedCount })
          : t("maintenance.generationDone", { count: result.generatedCount }),
      );
      await refresh();
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const createPending = useMutation({
    mutationFn: (input: {
      scheduledLocalDate: string;
      payload: PendingActivityPayloadInput;
      note: string | null;
    }) => unwrapResult(commands.createPendingActivity(input)),
    onSuccess: async () => {
      setShowPendingForm(false);
      setActionError(null);
      await queryClient.invalidateQueries({ queryKey: ["pending"] });
      await queryClient.invalidateQueries({ queryKey: ["maintenance"] });
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const updatePending = useMutation({
    mutationFn: (input: {
      id: string;
      scheduledLocalDate: string;
      payload: PendingActivityPayloadInput;
      note: string | null;
    }) => unwrapResult(commands.updatePendingActivity(input)),
    onSuccess: async () => {
      setEditingPending(null);
      setActionError(null);
      await queryClient.invalidateQueries({ queryKey: ["pending"] });
      await queryClient.invalidateQueries({ queryKey: ["maintenance"] });
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const preview = useMutation({
    mutationFn: (input: {
      id: string;
      localDate: string;
      localTime: string;
      ambiguousOffset: string | null;
    }) => unwrapResult(commands.previewPendingActivity(input)),
    onSuccess: (result) => {
      setPendingPreview(result.pending.id);
      setConfirmPost(result.pending.id);
      setActionError(null);
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const post = useMutation({
    mutationFn: (input: {
      id: string;
      localDate: string;
      localTime: string;
      ambiguousOffset: string | null;
    }) => unwrapResult(commands.postPendingActivity(input)),
    onSuccess: async () => {
      setConfirmPost(null);
      setPendingPreview(null);
      await refresh();
      await queryClient.invalidateQueries({ queryKey: ["overview"] });
      await queryClient.invalidateQueries({ queryKey: ["activities"] });
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const skip = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.skipPendingActivity({ id })),
    onSuccess: async () => {
      setConfirmSkip(null);
      await refresh();
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const archiveRule = useMutation({
    mutationFn: (id: string) =>
      unwrapResult(commands.archiveRecurringActivityRule({ id })),
    onSuccess: refresh,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const restoreRule = useMutation({
    mutationFn: (id: string) =>
      unwrapResult(commands.restoreRecurringActivityRule({ id })),
    onSuccess: refresh,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const snooze = useMutation({
    mutationFn: (input: {
      policyKind: string;
      targetAccountId: string | null;
      targetInstrumentId: string | null;
      targetCurrencyA: string | null;
      targetCurrencyB: string | null;
      snoozedUntil: string;
    }) => unwrapResult(commands.snoozeMaintenanceItem(input)),
    onSuccess: refresh,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const updatePolicy = useMutation({
    mutationFn: (input: {
      id: string | null;
      kind: string;
      targetAccountId: string | null;
      targetInstrumentId: string | null;
      targetCurrencyA: string | null;
      targetCurrencyB: string | null;
      reviewIntervalDays: number | null;
    }) => unwrapResult(commands.updateFreshnessPolicy(input)),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: ["freshness-policies"] });
      await queryClient.invalidateQueries({ queryKey: ["maintenance"] });
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  const error =
    actionError ??
    (maintenance.error ? commandErrorFromUnknown(maintenance.error) : null) ??
    (pending.error ? commandErrorFromUnknown(pending.error) : null);
  const accountItems = accounts.data ?? [];
  const pendingItems = pending.data?.items ?? [];
  const ruleItems = rules.data ?? [];
  const maintenanceItems = maintenance.data?.items ?? [];
  const policyItems = policies.data ?? [];

  return (
    <AppShell>
      <main className="mx-auto max-w-5xl px-8 py-10">
        <p className="mb-3 text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
          {t("maintenance.eyebrow")}
        </p>
        <div className="mb-8 flex flex-wrap items-center justify-between gap-3">
          <div>
            <h1 className="text-3xl font-semibold tracking-tight">
              {t("maintenance.title")}
            </h1>
            <p className="mt-2 max-w-2xl text-sm text-muted-foreground">
              {t("maintenance.description")}
            </p>
          </div>
          <div className="flex flex-wrap gap-2">
            <Button
              disabled={generate.isPending}
              onClick={() => generate.mutate()}
              type="button"
            >
              {generate.isPending
                ? t("maintenance.generating")
                : t("maintenance.generate")}
            </Button>
            <Button
              disabled={maintenance.isFetching}
              onClick={() => void refresh()}
              type="button"
              variant="ghost"
            >
              {t("maintenance.refresh")}
            </Button>
          </div>
        </div>
        {generationMessage ? (
          <p className="mb-4 text-sm text-muted-foreground" role="status">
            {generationMessage}
          </p>
        ) : null}
        {error ? (
          <p className="mb-4 text-sm text-destructive" role="alert">
            {formatCommandError(t, error)}
          </p>
        ) : null}
        <div className="space-y-8">
          <section aria-labelledby="maintenance-queue-title" className="space-y-3">
            <SectionHeading
              id="maintenance-queue-title"
              title={t("maintenance.queue")}
            />
            {maintenance.isPending ? (
              <p role="status">{t("references.loading")}</p>
            ) : null}
            {!maintenance.isPending && maintenanceItems.length === 0 ? (
              <p className="rounded-lg border border-dashed border-border p-6 text-sm text-muted-foreground">
                {t("maintenance.empty")}
              </p>
            ) : null}
            <div className="grid gap-3 md:grid-cols-2">
              {maintenanceItems.map((item) => (
                <MaintenanceCard
                  item={item}
                  key={item.id}
                  localDate={maintenance.data?.localDate ?? ""}
                  onSnooze={(until) => {
                    setActionError(null);
                    snooze.mutate({
                      policyKind: item.policyKind ?? "",
                      targetAccountId: item.targetAccountId,
                      targetInstrumentId: item.targetInstrumentId,
                      targetCurrencyA: item.targetCurrencyA,
                      targetCurrencyB: item.targetCurrencyB,
                      snoozedUntil: until,
                    });
                  }}
                  t={t}
                />
              ))}
            </div>
          </section>

          <section aria-labelledby="pending-title" className="space-y-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <SectionHeading id="pending-title" title={t("maintenance.pending")} />
              <Button
                onClick={() => {
                  setEditingPending(null);
                  setShowPendingForm((value) => !value);
                }}
                type="button"
              >
                {showPendingForm ? t("references.cancel") : t("maintenance.newPending")}
              </Button>
            </div>
            {showPendingForm ? (
              <PendingForm
                accounts={accountItems}
                catalogCurrencies={
                  catalog?.currencies.map((item) => item.value) ?? ["CNY"]
                }
                onCancel={() => setShowPendingForm(false)}
                onSubmit={(input) => createPending.mutate(input)}
                pending={null}
                saving={createPending.isPending}
                t={t}
              />
            ) : null}
            {editingPending ? (
              <PendingForm
                accounts={accountItems}
                catalogCurrencies={
                  catalog?.currencies.map((item) => item.value) ?? ["CNY"]
                }
                onCancel={() => setEditingPending(null)}
                onSubmit={(input) =>
                  updatePending.mutate({ id: editingPending.id, ...input })
                }
                pending={editingPending}
                saving={updatePending.isPending}
                t={t}
              />
            ) : null}
            {pending.isPending ? <p role="status">{t("references.loading")}</p> : null}
            {!pending.isPending && pendingItems.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                {t("maintenance.noPending")}
              </p>
            ) : null}
            <div className="space-y-2">
              {pendingItems.map((item) => (
                <PendingCard
                  confirmPost={confirmPost === item.id}
                  confirmSkip={confirmSkip === item.id}
                  item={item}
                  key={item.id}
                  onEdit={() => {
                    setShowPendingForm(false);
                    setEditingPending(item);
                  }}
                  onPost={(localDate, localTime) =>
                    post.mutate({
                      id: item.id,
                      localDate,
                      localTime,
                      ambiguousOffset: null,
                    })
                  }
                  onPreview={(localDate, localTime) =>
                    preview.mutate({
                      id: item.id,
                      localDate,
                      localTime,
                      ambiguousOffset: null,
                    })
                  }
                  onSkip={() => skip.mutate(item.id)}
                  onStartPost={() => setConfirmPost(item.id)}
                  onStartSkip={() => setConfirmSkip(item.id)}
                  pendingPreview={pendingPreview === item.id}
                  t={t}
                />
              ))}
            </div>
          </section>

          <section aria-labelledby="rules-title" className="space-y-3">
            <div className="flex flex-wrap items-center justify-between gap-2">
              <SectionHeading id="rules-title" title={t("maintenance.rules")} />
              <Button onClick={() => setShowRuleForm((value) => !value)} type="button">
                {showRuleForm ? t("references.cancel") : t("maintenance.newRule")}
              </Button>
            </div>
            {showRuleForm ? (
              <RecurringRuleForm
                accounts={accountItems}
                catalogCurrencies={
                  catalog?.currencies.map((item) => item.value) ?? ["CNY"]
                }
                onCancel={() => setShowRuleForm(false)}
                onSubmit={async (input) => {
                  try {
                    await unwrapResult(commands.createRecurringActivityRule(input));
                    setShowRuleForm(false);
                    await refresh();
                  } catch (caught) {
                    setActionError(commandErrorFromUnknown(caught));
                  }
                }}
                t={t}
              />
            ) : null}
            {rules.isPending ? <p role="status">{t("references.loading")}</p> : null}
            {!rules.isPending && ruleItems.length === 0 ? (
              <p className="text-sm text-muted-foreground">
                {t("maintenance.noRules")}
              </p>
            ) : null}
            <div className="space-y-2">
              {ruleItems.map((rule) => (
                <RuleCard
                  key={rule.id}
                  onArchive={() => archiveRule.mutate(rule.id)}
                  onRestore={() => restoreRule.mutate(rule.id)}
                  rule={rule}
                  t={t}
                />
              ))}
            </div>
          </section>

          <section aria-labelledby="policies-title" className="space-y-3">
            <SectionHeading id="policies-title" title={t("maintenance.policies")} />
            <p className="text-sm text-muted-foreground">
              {t("maintenance.policyDescription")}
            </p>
            <div className="grid gap-2 md:grid-cols-2">
              {policyItems.map((policy) => (
                <PolicyCard
                  key={policy.id}
                  onSave={(days) => updatePolicy.mutate(policyInput(policy, days))}
                  policy={policy}
                  t={t}
                />
              ))}
            </div>
          </section>
        </div>
      </main>
    </AppShell>
  );
}

function policyInput(policy: FreshnessPolicyDto, days: string) {
  const parsed = days.trim() === "" ? null : Number.parseInt(days, 10);
  return {
    id: policy.id,
    kind: policy.kind,
    targetAccountId: policy.targetAccountId,
    targetInstrumentId: policy.targetInstrumentId,
    targetCurrencyA: policy.targetCurrencyA,
    targetCurrencyB: policy.targetCurrencyB,
    reviewIntervalDays: Number.isFinite(parsed) ? parsed : null,
  };
}

function SectionHeading({ id, title }: { id: string; title: string }) {
  return (
    <h2 className="text-lg font-medium" id={id}>
      {title}
    </h2>
  );
}

function MaintenanceCard({
  item,
  localDate,
  onSnooze,
  t,
}: {
  item: MaintenanceItemDto;
  localDate: string;
  onSnooze: (until: string) => void;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const [until, setUntil] = useState(localDate);
  return (
    <article className="rounded-lg border border-border bg-card p-4 shadow-sm">
      <div className="flex items-start justify-between gap-3">
        <div>
          <h3 className="font-medium">{item.label}</h3>
          <p className="mt-1 text-sm text-muted-foreground">
            {item.itemKind} · {item.underlyingStatus}
          </p>
        </div>
        <span className="rounded-full bg-surface-soft px-2 py-1 text-xs font-medium">
          {item.status}
        </span>
      </div>
      {item.observedOn ? (
        <p className="mt-3 text-xs text-muted-foreground">
          {t("maintenance.observedOn", { date: item.observedOn })}
        </p>
      ) : null}
      {item.dueOn ? (
        <p className="text-xs text-muted-foreground">
          {t("maintenance.dueOn", { date: item.dueOn })}
        </p>
      ) : null}
      {item.pendingActivity ? (
        <p className="mt-2 text-sm">
          {t("maintenance.pendingLinked", {
            date: item.pendingActivity.scheduledLocalDate,
          })}
        </p>
      ) : null}
      {item.policyKind ? (
        <div className="mt-3 flex flex-wrap items-end gap-2">
          <label className="grid gap-1 text-xs font-medium">
            {t("maintenance.snoozeUntil")}
            <Input
              aria-label={t("maintenance.snoozeUntil")}
              onChange={(event) => setUntil(event.target.value)}
              type="date"
              value={until}
            />
          </label>
          <Button
            disabled={!until}
            onClick={() => onSnooze(until)}
            type="button"
            variant="ghost"
          >
            {t("maintenance.snooze")}
          </Button>
        </div>
      ) : null}
    </article>
  );
}

function PendingCard({
  item,
  confirmPost,
  confirmSkip,
  onEdit,
  onPost,
  onPreview,
  onSkip,
  onStartPost,
  onStartSkip,
  pendingPreview,
  t,
}: {
  item: PendingActivityDto;
  confirmPost: boolean;
  confirmSkip: boolean;
  onEdit: () => void;
  onPost: (date: string, time: string) => void;
  onPreview: (date: string, time: string) => void;
  onSkip: () => void;
  onStartPost: () => void;
  onStartSkip: () => void;
  pendingPreview: boolean;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  const [localTime, setLocalTime] = useState("12:00");
  return (
    <article className="rounded-lg border border-border bg-card p-4 shadow-sm">
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div>
          <h3 className="font-medium">{t(`activity.kinds.${item.payload.kind}`)}</h3>
          <p className="mt-1 text-sm text-muted-foreground">
            {item.scheduledLocalDate} · {item.creationSource}
          </p>
          {item.note ? <p className="mt-2 text-sm">{item.note}</p> : null}
        </div>
        <span className="rounded-full bg-surface-soft px-2 py-1 text-xs font-medium">
          {item.status}
        </span>
      </div>
      <div className="mt-4 flex flex-wrap items-end gap-2">
        <label className="grid gap-1 text-xs font-medium">
          {t("maintenance.effectiveTime")}
          <Input
            aria-label={t("maintenance.effectiveTime")}
            onChange={(event) => setLocalTime(event.target.value)}
            type="time"
            value={localTime}
          />
        </label>
        <Button
          onClick={() => onPreview(item.scheduledLocalDate, localTime)}
          type="button"
          variant="ghost"
        >
          {t("maintenance.preview")}
        </Button>
        <Button onClick={onEdit} type="button" variant="ghost">
          {t("references.edit")}
        </Button>
        <Button onClick={onStartSkip} type="button" variant="ghost">
          {t("maintenance.skip")}
        </Button>
        {pendingPreview ? (
          <Button onClick={onStartPost} type="button">
            {t("maintenance.post")}
          </Button>
        ) : null}
      </div>
      {confirmPost ? (
        <div
          className="mt-3 rounded-lg border border-border bg-surface-soft p-3"
          role="group"
        >
          <p className="text-sm">{t("maintenance.postConfirm")}</p>
          <div className="mt-2 flex gap-2">
            <Button
              autoFocus
              onClick={() => onPost(item.scheduledLocalDate, localTime)}
              type="button"
            >
              {t("maintenance.confirmPost")}
            </Button>
            <Button onClick={() => onStartPost()} type="button" variant="ghost">
              {t("references.cancel")}
            </Button>
          </div>
        </div>
      ) : null}
      {confirmSkip ? (
        <div
          className="mt-3 rounded-lg border border-destructive/40 bg-destructive/5 p-3"
          role="group"
        >
          <p className="text-sm">{t("maintenance.skipConfirm")}</p>
          <div className="mt-2 flex gap-2">
            <Button autoFocus onClick={onSkip} type="button" variant="destructive">
              {t("maintenance.confirmSkip")}
            </Button>
            <Button onClick={onStartSkip} type="button" variant="ghost">
              {t("references.cancel")}
            </Button>
          </div>
        </div>
      ) : null}
    </article>
  );
}

function RuleCard({
  rule,
  onArchive,
  onRestore,
  t,
}: {
  rule: RecurringActivityRuleDto;
  onArchive: () => void;
  onRestore: () => void;
  t: (key: string) => string;
}) {
  return (
    <article className="flex flex-wrap items-center justify-between gap-3 rounded-lg border border-border bg-card p-4 shadow-sm">
      <div>
        <h3 className="font-medium">{t(`activity.kinds.${rule.payload.kind}`)}</h3>
        <p className="mt-1 text-sm text-muted-foreground">
          {rule.cadence} × {rule.intervalValue} · {rule.startLocalDate}
          {rule.endLocalDate ? ` → ${rule.endLocalDate}` : ""}
        </p>
        {rule.note ? <p className="mt-1 text-sm">{rule.note}</p> : null}
      </div>
      <div className="flex items-center gap-2">
        {rule.archivedAt ? (
          <span className="rounded-full bg-surface-soft px-2 py-1 text-xs">
            {t("references.archived")}
          </span>
        ) : null}
        {rule.archivedAt ? (
          <Button onClick={onRestore} type="button" variant="ghost">
            {t("references.restore")}
          </Button>
        ) : (
          <Button onClick={onArchive} type="button" variant="ghost">
            {t("references.archive")}
          </Button>
        )}
      </div>
    </article>
  );
}

function PolicyCard({
  policy,
  onSave,
  t,
}: {
  policy: FreshnessPolicyDto;
  onSave: (days: string) => void;
  t: (key: string) => string;
}) {
  const [days, setDays] = useState(policy.reviewIntervalDays?.toString() ?? "");
  return (
    <article className="rounded-lg border border-border bg-card p-4 shadow-sm">
      <h3 className="font-medium">{policy.kind}</h3>
      <p className="mt-1 text-xs text-muted-foreground">
        {policy.isDefault
          ? t("maintenance.defaultPolicy")
          : t("maintenance.overridePolicy")}
      </p>
      <div className="mt-3 flex items-end gap-2">
        <label className="grid flex-1 gap-1 text-xs font-medium">
          {t("maintenance.reviewDays")}
          <Input
            min={1}
            max={3650}
            onChange={(event) => setDays(event.target.value)}
            type="number"
            value={days}
          />
        </label>
        <Button onClick={() => onSave(days)} type="button" variant="ghost">
          {t("references.save")}
        </Button>
      </div>
    </article>
  );
}

function PendingForm({
  accounts,
  catalogCurrencies,
  pending,
  onCancel,
  onSubmit,
  saving,
  t,
}: {
  accounts: AccountRecordDto[];
  catalogCurrencies: string[];
  pending: PendingActivityDto | null;
  onCancel: () => void;
  onSubmit: (input: {
    scheduledLocalDate: string;
    payload: PendingActivityPayloadInput;
    note: string | null;
  }) => void;
  saving: boolean;
  t: (key: string) => string;
}) {
  const initialKind = (pending?.payload.kind ?? "deposit") as PayloadKind;
  const [kind, setKind] = useState<PayloadKind>(initialKind);
  const [values, setValues] = useState<FieldValues>(() => ({
    ...EMPTY_VALUES,
    ...valuesFromPayload(pending?.payload),
  }));
  const [date, setDate] = useState(pending?.scheduledLocalDate ?? "");
  const [note, setNote] = useState(pending?.note ?? "");
  const [formError, setFormError] = useState<string | null>(null);
  const set = (name: string, value: string) =>
    setValues((previous) => ({ ...previous, [name]: value }));
  function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      const payload = payloadFromValues(kind, values);
      if (!date) throw new Error(t("maintenance.dateRequired"));
      setFormError(null);
      onSubmit({ scheduledLocalDate: date, payload, note: note.trim() || null });
    } catch (error) {
      setFormError(
        error instanceof Error ? error.message : t("maintenance.formInvalid"),
      );
    }
  }
  return (
    <form
      className="rounded-lg border border-border bg-card p-4 shadow-sm"
      onSubmit={submit}
    >
      <div className="grid gap-3 md:grid-cols-3">
        <Field label={t("maintenance.kind")}>
          <select
            aria-label={t("maintenance.kind")}
            className={selectClass}
            onChange={(event) => setKind(event.target.value as PayloadKind)}
            value={kind}
          >
            {PAYLOAD_KINDS.map((value) => (
              <option key={value} value={value}>
                {t(`activity.kinds.${value}`)}
              </option>
            ))}
          </select>
        </Field>
        <Field label={t("maintenance.scheduledDate")}>
          <Input
            aria-label={t("maintenance.scheduledDate")}
            onChange={(event) => setDate(event.target.value)}
            required
            type="date"
            value={date}
          />
        </Field>
        <Field label={t("references.note")}>
          <Input
            aria-label={t("references.note")}
            onChange={(event) => setNote(event.target.value)}
            value={note}
          />
        </Field>
      </div>
      <PayloadFields
        accounts={accounts}
        currencies={catalogCurrencies}
        kind={kind}
        set={set}
        t={t}
        values={values}
      />
      {formError ? (
        <p className="mt-3 text-sm text-destructive" role="alert">
          {formError}
        </p>
      ) : null}
      <div className="mt-4 flex gap-2">
        <Button disabled={saving} type="submit">
          {saving ? t("references.saving") : t("references.save")}
        </Button>
        <Button onClick={onCancel} type="button" variant="ghost">
          {t("references.cancel")}
        </Button>
      </div>
    </form>
  );
}

function RecurringRuleForm({
  accounts,
  catalogCurrencies,
  onCancel,
  onSubmit,
  t,
}: {
  accounts: AccountRecordDto[];
  catalogCurrencies: string[];
  onCancel: () => void;
  onSubmit: (input: {
    cadence: string;
    intervalValue: number;
    startLocalDate: string;
    endLocalDate: string | null;
    payload: PendingActivityPayloadInput;
    note: string | null;
  }) => void;
  t: (key: string) => string;
}) {
  const [cadence, setCadence] = useState("monthly");
  const [intervalValue, setIntervalValue] = useState("1");
  const [startDate, setStartDate] = useState("");
  const [endDate, setEndDate] = useState("");
  const [kind, setKind] = useState<PayloadKind>("deposit");
  const [values, setValues] = useState<FieldValues>({ ...EMPTY_VALUES });
  const [note, setNote] = useState("");
  const [formError, setFormError] = useState<string | null>(null);
  const set = (name: string, value: string) =>
    setValues((previous) => ({ ...previous, [name]: value }));
  function submit(event: React.FormEvent<HTMLFormElement>) {
    event.preventDefault();
    try {
      if (!startDate) throw new Error(t("maintenance.dateRequired"));
      const payload = payloadFromValues(kind, values);
      onSubmit({
        cadence,
        intervalValue: Number.parseInt(intervalValue, 10),
        startLocalDate: startDate,
        endLocalDate: endDate || null,
        payload,
        note: note.trim() || null,
      });
      setFormError(null);
    } catch (error) {
      setFormError(
        error instanceof Error ? error.message : t("maintenance.formInvalid"),
      );
    }
  }
  return (
    <form
      className="rounded-lg border border-border bg-card p-4 shadow-sm"
      onSubmit={submit}
    >
      <div className="grid gap-3 md:grid-cols-4">
        <Field label={t("maintenance.cadence")}>
          <select
            className={selectClass}
            onChange={(event) => setCadence(event.target.value)}
            value={cadence}
          >
            <option value="daily">{t("maintenance.cadences.daily")}</option>
            <option value="weekly">{t("maintenance.cadences.weekly")}</option>
            <option value="monthly">{t("maintenance.cadences.monthly")}</option>
            <option value="yearly">{t("maintenance.cadences.yearly")}</option>
          </select>
        </Field>
        <Field label={t("maintenance.interval")}>
          <Input
            min={1}
            onChange={(event) => setIntervalValue(event.target.value)}
            type="number"
            value={intervalValue}
          />
        </Field>
        <Field label={t("maintenance.startDate")}>
          <Input
            onChange={(event) => setStartDate(event.target.value)}
            required
            type="date"
            value={startDate}
          />
        </Field>
        <Field label={t("maintenance.endDate")}>
          <Input
            onChange={(event) => setEndDate(event.target.value)}
            type="date"
            value={endDate}
          />
        </Field>
      </div>
      <div className="mt-3 grid gap-3 md:grid-cols-2">
        <Field label={t("maintenance.kind")}>
          <select
            className={selectClass}
            onChange={(event) => setKind(event.target.value as PayloadKind)}
            value={kind}
          >
            {PAYLOAD_KINDS.filter((value) =>
              [
                "deposit",
                "withdrawal",
                "transfer",
                "income",
                "fee",
                "debt_draw",
                "debt_payment",
              ].includes(value),
            ).map((value) => (
              <option key={value} value={value}>
                {t(`activity.kinds.${value}`)}
              </option>
            ))}
          </select>
        </Field>
        <Field label={t("references.note")}>
          <Input onChange={(event) => setNote(event.target.value)} value={note} />
        </Field>
      </div>
      <PayloadFields
        accounts={accounts}
        currencies={catalogCurrencies}
        kind={kind}
        set={set}
        t={t}
        values={values}
      />
      {formError ? (
        <p className="mt-3 text-sm text-destructive" role="alert">
          {formError}
        </p>
      ) : null}
      <div className="mt-4 flex gap-2">
        <Button type="submit">{t("references.save")}</Button>
        <Button onClick={onCancel} type="button" variant="ghost">
          {t("references.cancel")}
        </Button>
      </div>
    </form>
  );
}

function PayloadFields({
  accounts,
  currencies,
  kind,
  set,
  t,
  values,
}: {
  accounts: AccountRecordDto[];
  currencies: string[];
  kind: PayloadKind;
  set: (name: string, value: string) => void;
  t: (key: string) => string;
  values: FieldValues;
}) {
  const accountSelect = (name: string, label: string) => (
    <Field label={label}>
      <select
        aria-label={label}
        className={selectClass}
        onChange={(event) => set(name, event.target.value)}
        value={values[name] ?? ""}
      >
        <option value="">{t("maintenance.selectAccount")}</option>
        {accounts.map((account) => (
          <option key={account.id} value={account.id}>
            {account.name}
          </option>
        ))}
      </select>
    </Field>
  );
  const currency = (name: string, label: string) => (
    <Field label={label}>
      <select
        aria-label={label}
        className={selectClass}
        onChange={(event) => set(name, event.target.value)}
        value={values[name] ?? currencies[0] ?? "CNY"}
      >
        {currencies.map((value) => (
          <option key={value} value={value}>
            {value}
          </option>
        ))}
      </select>
    </Field>
  );
  const text = (name: string, label: string, type = "text") => (
    <Field label={label}>
      <Input
        aria-label={label}
        onChange={(event) => set(name, event.target.value)}
        required={!["feeAmount", "fxRate", "instrumentIdOptional"].includes(name)}
        type={type}
        value={values[name] ?? ""}
      />
    </Field>
  );
  if (kind === "deposit" || kind === "withdrawal")
    return (
      <div className="mt-3 grid gap-3 md:grid-cols-4">
        {accountSelect("accountId", t("maintenance.account"))}
        <Field label={t("maintenance.component")}>
          <select
            className={selectClass}
            onChange={(event) => set("component", event.target.value)}
            value={values.component}
          >
            <option value="account_value">{t("maintenance.accountValue")}</option>
            <option value="holdings_cash">{t("maintenance.holdingsCash")}</option>
          </select>
        </Field>
        {text("amount", t("maintenance.amount"), "text")}
        {currency("currency", t("maintenance.currency"))}
      </div>
    );
  if (kind === "transfer")
    return (
      <div className="mt-3 grid gap-3 md:grid-cols-4">
        {accountSelect("sourceAccountId", t("maintenance.sourceAccount"))}
        <Field label={t("maintenance.sourceComponent")}>
          <select
            className={selectClass}
            onChange={(event) => set("sourceComponent", event.target.value)}
            value={values.sourceComponent}
          >
            <option value="account_value">{t("maintenance.accountValue")}</option>
            <option value="holdings_cash">{t("maintenance.holdingsCash")}</option>
          </select>
        </Field>
        {text("sourceAmount", t("maintenance.sourceAmount"))}
        {currency("sourceCurrency", t("maintenance.sourceCurrency"))}
        {accountSelect("destinationAccountId", t("maintenance.destinationAccount"))}
        {Field({
          label: t("maintenance.destinationComponent"),
          children: (
            <select
              className={selectClass}
              onChange={(event) => set("destinationComponent", event.target.value)}
              value={values.destinationComponent}
            >
              <option value="account_value">{t("maintenance.accountValue")}</option>
              <option value="holdings_cash">{t("maintenance.holdingsCash")}</option>
            </select>
          ),
        })}
        {text("destinationAmount", t("maintenance.destinationAmount"))}
        {currency("destinationCurrency", t("maintenance.destinationCurrency"))}
        {text("feeAmount", t("maintenance.feeAmount"))}
        {text("feeKind", t("maintenance.feeKind"))}
      </div>
    );
  if (kind === "position_transfer")
    return (
      <div className="mt-3 grid gap-3 md:grid-cols-3">
        {text("sourceHoldingId", t("maintenance.sourceHoldingId"))}
        {text("destinationHoldingId", t("maintenance.destinationHoldingId"))}
        {text("quantity", t("maintenance.quantity"))}
      </div>
    );
  if (kind === "buy" || kind === "sell")
    return (
      <div className="mt-3 grid gap-3 md:grid-cols-4">
        {text("holdingId", t("maintenance.holdingId"))}
        {text("instrumentId", t("maintenance.instrumentId"))}
        {text("quantity", t("maintenance.quantity"))}
        {text("unitPrice", t("maintenance.unitPrice"))}
        {text("grossAmount", t("maintenance.grossAmount"))}
        {currency("settlementCurrency", t("maintenance.currency"))}
        {text("feeAmount", t("maintenance.feeAmount"))}
      </div>
    );
  if (kind === "income" || kind === "fee")
    return (
      <div className="mt-3 grid gap-3 md:grid-cols-4">
        {accountSelect("accountId", t("maintenance.account"))}
        {Field({
          label: t("maintenance.component"),
          children: (
            <select
              className={selectClass}
              onChange={(event) => set("component", event.target.value)}
              value={values.component}
            >
              <option value="account_value">{t("maintenance.accountValue")}</option>
              <option value="holdings_cash">{t("maintenance.holdingsCash")}</option>
            </select>
          ),
        })}
        {text("amount", t("maintenance.amount"))}
        {currency("currency", t("maintenance.currency"))}
        {text(kind === "income" ? "incomeKind" : "feeKind", t("maintenance.kind"))}
        {text("instrumentIdOptional", t("maintenance.instrumentOptional"))}
      </div>
    );
  return (
    <div className="mt-3 grid gap-3 md:grid-cols-4">
      {accountSelect("liabilityAccountId", t("maintenance.liabilityAccount"))}
      {text("principalAmount", t("maintenance.principalAmount"))}
      {currency("principalCurrency", t("maintenance.principalCurrency"))}
      {accountSelect("cashAccountId", t("maintenance.cashAccount"))}
      {text("cashAmount", t("maintenance.cashAmount"))}
      {currency("cashCurrency", t("maintenance.cashCurrency"))}
      {text("fxRate", t("maintenance.fxRate"))}
    </div>
  );
}

function payloadFromValues(
  kind: PayloadKind,
  values: FieldValues,
): PendingActivityPayloadInput {
  const required = (name: string) => {
    const value = values[name]?.trim();
    if (!value) throw new Error(`${name} is required`);
    return value;
  };
  const optional = (name: string) => values[name]?.trim() || null;
  if (kind === "deposit" || kind === "withdrawal")
    return {
      kind,
      accountId: required("accountId"),
      component: required("component"),
      amount: required("amount"),
      currency: required("currency"),
    };
  if (kind === "transfer")
    return {
      kind,
      sourceAccountId: required("sourceAccountId"),
      sourceComponent: required("sourceComponent"),
      sourceAmount: required("sourceAmount"),
      sourceCurrency: required("sourceCurrency"),
      destinationAccountId: required("destinationAccountId"),
      destinationComponent: required("destinationComponent"),
      destinationAmount: required("destinationAmount"),
      destinationCurrency: required("destinationCurrency"),
      feeAmount: optional("feeAmount"),
      feeKind: optional("feeKind"),
    };
  if (kind === "position_transfer")
    return {
      kind,
      sourceHoldingId: required("sourceHoldingId"),
      destinationHoldingId: required("destinationHoldingId"),
      quantity: required("quantity"),
    };
  if (kind === "buy" || kind === "sell")
    return {
      kind,
      holdingId: required("holdingId"),
      instrumentId: required("instrumentId"),
      quantity: required("quantity"),
      unitPrice: required("unitPrice"),
      grossAmount: required("grossAmount"),
      settlementCurrency: required("settlementCurrency"),
      feeAmount: optional("feeAmount"),
      confirmZeroUnitPrice: false,
    };
  if (kind === "income")
    return {
      kind,
      accountId: required("accountId"),
      component: required("component"),
      amount: required("amount"),
      currency: required("currency"),
      incomeKind: required("incomeKind"),
      instrumentId: optional("instrumentIdOptional"),
    };
  if (kind === "fee")
    return {
      kind,
      accountId: required("accountId"),
      component: required("component"),
      amount: required("amount"),
      currency: required("currency"),
      feeKind: required("feeKind"),
      instrumentId: optional("instrumentIdOptional"),
    };
  if (kind === "debt_draw")
    return {
      kind,
      liabilityAccountId: required("liabilityAccountId"),
      principalAmount: required("principalAmount"),
      principalCurrency: required("principalCurrency"),
      cashAccountId: optional("cashAccountId"),
      cashComponent: optional("cashComponent"),
      cashAmount: optional("cashAmount"),
      cashCurrency: optional("cashCurrency"),
      fxRate: optional("fxRate"),
    };
  return {
    kind: "debt_payment",
    liabilityAccountId: required("liabilityAccountId"),
    principalAmount: required("principalAmount"),
    principalCurrency: required("principalCurrency"),
    cashAccountId: required("cashAccountId"),
    cashComponent: required("cashComponent"),
    cashAmount: required("cashAmount"),
    cashCurrency: required("cashCurrency"),
    fxRate: optional("fxRate"),
    feeAmount: optional("feeAmount"),
    feeKind: optional("feeKind"),
  };
}

function valuesFromPayload(
  payload: PendingActivityPayloadInput | undefined,
): FieldValues {
  if (!payload) return {};
  const values: FieldValues = { ...payload } as unknown as FieldValues;
  if (payload.kind === "income" || payload.kind === "fee")
    values.instrumentIdOptional = payload.instrumentId ?? "";
  return values;
}

function Field({ label, children }: { label: string; children: React.ReactNode }) {
  return (
    <label className="grid gap-1 text-xs font-medium">
      {label}
      {children}
    </label>
  );
}
const selectClass =
  "h-10 w-full rounded-lg border border-border bg-card px-3 text-sm font-normal text-foreground shadow-sm outline-none focus-visible:ring-2 focus-visible:ring-ring";

export default MaintenancePage;
