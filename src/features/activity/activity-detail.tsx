import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useRef, useState } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  ActivityBadges,
  ActivityLegs,
  FxConversionPanel,
  activityKindLabel,
  classificationLabel,
} from "@/features/activity/activity-display";
import { ActivityForm } from "@/features/activity/activity-form";
import { GhostButton } from "@/features/references/reference-page";
import {
  commands,
  type AccountRecordDto,
  type ActivityDetailDto,
  type CommandError,
  type InstrumentRecordDto,
  type ReferenceCatalogDto,
} from "@/generated/tauri-bindings";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { invalidateValuation } from "@/lib/tauri/invalidate";

export function ActivityDetailPanel({
  accounts,
  activityId,
  catalog,
  instruments,
  timezoneConfirmed,
  onClose,
}: {
  accounts: AccountRecordDto[];
  activityId: string;
  catalog: ReferenceCatalogDto;
  instruments: InstrumentRecordDto[];
  timezoneConfirmed: boolean;
  onClose: () => void;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const reverseTriggerRef = useRef<HTMLButtonElement>(null);
  const correctTriggerRef = useRef<HTMLButtonElement>(null);
  const [actionError, setActionError] = useState<CommandError | null>(null);
  const [confirmingReverse, setConfirmingReverse] = useState(false);
  const [correcting, setCorrecting] = useState(false);
  const detail = useQuery({
    queryKey: ["activity", activityId],
    queryFn: () => unwrapResult(commands.getActivity({ id: activityId })),
  });
  const reverse = useMutation({
    mutationFn: () =>
      unwrapResult(
        commands.reverseActivity({
          id: activityId,
          localDate: null,
          localTime: null,
          ambiguousOffset: null,
        }),
      ),
    onSuccess: async () => {
      setConfirmingReverse(false);
      await invalidateValuation(queryClient);
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  const activity = detail.data;
  const error = detail.error ? commandErrorFromUnknown(detail.error) : actionError;
  const archivedReference = Boolean(
    activity?.legs.some((leg) =>
      accounts.some((account) => account.id === leg.accountId && account.archivedAt),
    ),
  );

  return (
    <section
      aria-label={t("activity.detail")}
      className="space-y-4 rounded-xl border border-border bg-card px-4 py-4 shadow-sm"
    >
      <div className="flex flex-wrap items-start justify-between gap-2">
        <h2 className="text-lg font-medium">{t("activity.detail")}</h2>
        <GhostButton onClick={onClose} type="button">
          {t("activity.closeDetail")}
        </GhostButton>
      </div>
      {detail.isPending ? <p role="status">{t("references.loading")}</p> : null}
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, error)}
        </p>
      ) : null}
      {activity ? (
        <>
          <ActivitySummary
            activity={activity}
            archivedReference={archivedReference}
            catalog={catalog}
          />
          {activity.reversed || activity.isReversal ? null : (
            <div className="flex flex-wrap gap-2">
              <Button
                disabled={!timezoneConfirmed || reverse.isPending}
                onClick={() => {
                  setActionError(null);
                  setConfirmingReverse(true);
                }}
                ref={reverseTriggerRef}
                type="button"
                variant="ghost"
              >
                {t("activity.reverse")}
              </Button>
              <Button
                disabled={!timezoneConfirmed}
                onClick={() => {
                  setActionError(null);
                  setCorrecting(true);
                }}
                ref={correctTriggerRef}
                type="button"
                variant="ghost"
              >
                {t("activity.correct")}
              </Button>
            </div>
          )}
          {confirmingReverse ? (
            <div
              className="space-y-3 rounded-lg border border-border px-4 py-3"
              role="group"
            >
              <p className="font-medium">{t("activity.confirmReverseTitle")}</p>
              <p className="text-sm text-muted-foreground">
                {t("activity.confirmReverseDescription")}
              </p>
              <div className="flex flex-wrap gap-2">
                <Button
                  autoFocus
                  disabled={reverse.isPending}
                  onClick={() => reverse.mutate()}
                  type="button"
                >
                  {reverse.isPending
                    ? t("activity.reversing")
                    : t("activity.confirmReverse")}
                </Button>
                <GhostButton
                  disabled={reverse.isPending}
                  onClick={() => {
                    setConfirmingReverse(false);
                    reverseTriggerRef.current?.focus();
                  }}
                  type="button"
                >
                  {t("references.cancel")}
                </GhostButton>
              </div>
            </div>
          ) : null}
          {correcting ? (
            <ActivityForm
              accounts={accounts}
              catalog={catalog}
              defaultCurrency={activity.legs[0]?.currency ?? "CNY"}
              instruments={instruments}
              mode="correction"
              originalId={activity.id}
              timezoneConfirmed={timezoneConfirmed}
              onCancel={() => {
                setCorrecting(false);
                correctTriggerRef.current?.focus();
              }}
              onPosted={async () => {
                setCorrecting(false);
                await queryClient.invalidateQueries({
                  queryKey: ["activity", activityId],
                });
              }}
            />
          ) : null}
        </>
      ) : null}
    </section>
  );
}

function ActivitySummary({
  activity,
  archivedReference,
  catalog,
}: {
  activity: ActivityDetailDto;
  archivedReference: boolean;
  catalog: ReferenceCatalogDto;
}) {
  const { t } = useTranslation();
  return (
    <div className="space-y-3">
      <div>
        <p className="font-medium">{activityKindLabel(t, activity.kind)}</p>
        <p className="text-sm text-muted-foreground">
          {classificationLabel(t, activity.classification)} ·{" "}
          {activity.effectiveLocalDate}
        </p>
        <ActivityBadges activity={activity} archivedReference={archivedReference} />
      </div>
      {activity.note ? <p className="text-sm">{activity.note}</p> : null}
      <dl className="grid gap-2 text-sm sm:grid-cols-2">
        <div>
          <dt className="text-muted-foreground">{t("activity.effectiveLocalDate")}</dt>
          <dd>{activity.effectiveLocalDate}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">{t("activity.createdAt")}</dt>
          <dd>{activity.createdAt}</dd>
        </div>
        {activity.incomeKind ? (
          <div>
            <dt className="text-muted-foreground">{t("activity.incomeKind")}</dt>
            <dd>
              {t(`activity.incomeKinds.${activity.incomeKind}`, {
                defaultValue: activity.incomeKind,
              })}
            </dd>
          </div>
        ) : null}
        {activity.feeKind ? (
          <div>
            <dt className="text-muted-foreground">{t("activity.feeKind")}</dt>
            <dd>
              {t(`activity.feeKinds.${activity.feeKind}`, {
                defaultValue: activity.feeKind,
              })}
            </dd>
          </div>
        ) : null}
      </dl>
      <ActivityLegs catalog={catalog} legs={activity.legs} />
      <FxConversionPanel catalog={catalog} conversion={activity.fxConversion} />
      <CorrectionChain activity={activity} />
    </div>
  );
}

function CorrectionChain({ activity }: { activity: ActivityDetailDto }) {
  const { t } = useTranslation();
  const chain = activity.chain;
  if (!chain.reversalId && !chain.replacementId) {
    return null;
  }
  return (
    <div>
      <h3 className="text-sm font-medium">{t("activity.chain")}</h3>
      <ul className="mt-2 space-y-1 text-sm text-muted-foreground">
        <li>
          {t("activity.original")}: {chain.originalId}
        </li>
        {chain.reversalId ? (
          <li>
            {t("activity.reversalRecord")}: {chain.reversalId}
          </li>
        ) : null}
        {chain.replacementId ? (
          <li>
            {t("activity.replacement")}: {chain.replacementId}
          </li>
        ) : null}
      </ul>
    </div>
  );
}
