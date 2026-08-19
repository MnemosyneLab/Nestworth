import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useEffect, useId, useRef, useState } from "react";
import { useForm } from "react-hook-form";
import { useTranslation } from "react-i18next";
import { z } from "zod";

import { AMOUNT_PATTERN, DATE_PATTERN } from "@/features/accounts/schema";
import { invalidateAnalyticsQueries } from "@/features/analytics/model";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import { Textarea } from "@/components/ui/textarea";
import {
  applyServerFieldErrors,
  applyZodIssues,
  FieldError,
} from "@/features/references/form-helpers";
import { GhostButton } from "@/features/references/reference-page";
import {
  commands,
  type CommandError,
  type HoldingLotDto,
} from "@/generated/tauri-bindings";
import { emptyToNull } from "@/lib/empty-to-null";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";

const DECLARE_FIELDS = ["declaredCost", "acquiredOn", "note"] as const;

const declareSchema = z.object({
  declaredCost: z
    .string()
    .trim()
    .min(1, "required")
    .regex(AMOUNT_PATTERN, "amount"),
  acquiredOn: z
    .string()
    .trim()
    .refine((value) => value === "" || DATE_PATTERN.test(value), "date"),
  note: z.string(),
});

type DeclareFormValues = z.infer<typeof declareSchema>;

export function DeclarationForm({
  accountName,
  instrumentName,
  lot,
  onCancel,
  quoteCurrency,
}: {
  accountName: string;
  instrumentName: string;
  lot: HoldingLotDto;
  onCancel: () => void;
  quoteCurrency: string;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const formId = useId();
  const confirmRef = useRef<HTMLButtonElement>(null);
  const submitRef = useRef<HTMLButtonElement>(null);
  const [confirming, setConfirming] = useState(false);
  const [actionError, setActionError] = useState<CommandError | null>(null);
  const form = useForm<DeclareFormValues>({
    defaultValues: { declaredCost: "", acquiredOn: "", note: "" },
  });

  useEffect(() => {
    if (confirming) {
      confirmRef.current?.focus();
    }
  }, [confirming]);

  const declareLot = useMutation({
    mutationFn: (values: DeclareFormValues) =>
      unwrapResult(
        commands.declareLotCostBasis({
          lotRef: lot.lotRef,
          instrumentId: lot.instrumentId,
          declaredCost: values.declaredCost.trim(),
          declaredCurrency: quoteCurrency,
          acquiredOn: emptyToNull(values.acquiredOn),
          note: emptyToNull(values.note),
        }),
      ),
    onSuccess: async () => {
      await invalidateAnalyticsQueries(queryClient);
      onCancel();
    },
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setActionError(commandError);
      applyServerFieldErrors(form, commandError.fields, [...DECLARE_FIELDS]);
    },
  });

  function onSubmit(values: DeclareFormValues) {
    const parsed = declareSchema.safeParse(values);
    if (!parsed.success) {
      applyZodIssues(form, parsed.error.issues, [...DECLARE_FIELDS]);
      return;
    }
    setActionError(null);
    setConfirming(true);
  }

  function translateField(message: string | undefined): string | undefined {
    if (!message) {
      return undefined;
    }
    if (message === "required" || message === "amount" || message === "date") {
      return t(`analytics.errors.${message}`);
    }
    return message;
  }

  return (
    <form
      aria-labelledby={`${formId}-title`}
      className="space-y-4 rounded-xl border border-border bg-card px-4 py-4"
      onSubmit={form.handleSubmit(onSubmit)}
    >
      <h3 className="text-lg font-medium" id={`${formId}-title`}>
        {t("analytics.declare.title")}
      </h3>
      <LotFacts
        accountName={accountName}
        instrumentName={instrumentName}
        lot={lot}
        quoteCurrency={quoteCurrency}
      />
      <p className="text-sm text-muted-foreground">{t("analytics.declare.noChange")}</p>
      <p className="text-sm text-muted-foreground">
        {t("analytics.declare.appendOnly")}
      </p>
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("analytics.declare.cost")}
        <Input
          autoComplete="off"
          disabled={declareLot.isPending}
          {...form.register("declaredCost")}
        />
        <FieldError message={translateField(form.formState.errors.declaredCost?.message)} />
      </label>
      <p className="text-sm text-muted-foreground">
        {t("analytics.declare.currency")}: {quoteCurrency}
      </p>
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("analytics.declare.acquiredOn")}
        <Input
          autoComplete="off"
          disabled={declareLot.isPending}
          {...form.register("acquiredOn")}
        />
        <span>{t("analytics.declare.acquiredOnHelp")}</span>
        <FieldError message={translateField(form.formState.errors.acquiredOn?.message)} />
      </label>
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("analytics.declare.note")}
        <Textarea disabled={declareLot.isPending} {...form.register("note")} />
      </label>
      {actionError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, actionError)}
        </p>
      ) : null}
      {confirming ? (
        <div
          aria-labelledby={`${formId}-confirm-title`}
          className="space-y-3 rounded-lg border border-border px-4 py-3"
          role="group"
        >
          <p className="font-medium" id={`${formId}-confirm-title`}>
            {t("analytics.declare.confirmTitle")}
          </p>
          <p className="text-sm text-muted-foreground">
            {t("analytics.declare.confirmDescription")}
          </p>
          <div className="flex flex-wrap gap-2">
            <Button
              disabled={declareLot.isPending}
              onClick={() => {
                setActionError(null);
                declareLot.mutate(form.getValues());
              }}
              ref={confirmRef}
              type="button"
            >
              {declareLot.isPending
                ? t("analytics.declare.submitting")
                : t("analytics.declare.confirm")}
            </Button>
            <GhostButton
              disabled={declareLot.isPending}
              onClick={() => {
                setConfirming(false);
                submitRef.current?.focus();
              }}
              type="button"
            >
              {t("references.cancel")}
            </GhostButton>
          </div>
        </div>
      ) : (
        <div className="flex flex-wrap gap-2">
          <Button disabled={declareLot.isPending} ref={submitRef} type="submit">
            {t("analytics.declare.action")}
          </Button>
          <GhostButton
            disabled={declareLot.isPending}
            onClick={onCancel}
            type="button"
          >
            {t("references.cancel")}
          </GhostButton>
        </div>
      )}
    </form>
  );
}

export function RevocationForm({
  accountName,
  instrumentName,
  lot,
  onCancel,
  quoteCurrency,
}: {
  accountName: string;
  instrumentName: string;
  lot: HoldingLotDto;
  onCancel: () => void;
  quoteCurrency: string;
}) {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const formId = useId();
  const confirmRef = useRef<HTMLButtonElement>(null);
  const [actionError, setActionError] = useState<CommandError | null>(null);
  const revokeLot = useMutation({
    mutationFn: () =>
      unwrapResult(commands.revokeLotCostBasis({ lotRef: lot.lotRef })),
    onSuccess: async () => {
      await invalidateAnalyticsQueries(queryClient);
      onCancel();
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  useEffect(() => {
    confirmRef.current?.focus();
  }, []);

  return (
    <div
      aria-labelledby={`${formId}-title`}
      className="space-y-4 rounded-xl border border-border bg-card px-4 py-4"
      role="group"
    >
      <h3 className="text-lg font-medium" id={`${formId}-title`}>
        {t("analytics.revoke.title")}
      </h3>
      <LotFacts
        accountName={accountName}
        instrumentName={instrumentName}
        lot={lot}
        quoteCurrency={quoteCurrency}
      />
      <p className="text-sm text-muted-foreground">
        {t("analytics.revoke.returnsUnknown")}
      </p>
      <p className="font-medium" id={`${formId}-confirm-title`}>
        {t("analytics.revoke.confirmTitle")}
      </p>
      <p className="text-sm text-muted-foreground">
        {t("analytics.revoke.confirmDescription")}
      </p>
      {actionError ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, actionError)}
        </p>
      ) : null}
      <div className="flex flex-wrap gap-2">
        <Button
          disabled={revokeLot.isPending}
          onClick={() => {
            setActionError(null);
            revokeLot.mutate();
          }}
          ref={confirmRef}
          type="button"
        >
          {revokeLot.isPending
            ? t("analytics.revoke.submitting")
            : t("analytics.revoke.confirm")}
        </Button>
        <GhostButton
          disabled={revokeLot.isPending}
          onClick={onCancel}
          type="button"
        >
          {t("references.cancel")}
        </GhostButton>
      </div>
    </div>
  );
}

function LotFacts({
  accountName,
  instrumentName,
  lot,
  quoteCurrency,
}: {
  accountName: string;
  instrumentName: string;
  lot: HoldingLotDto;
  quoteCurrency: string;
}) {
  const { t } = useTranslation();
  return (
    <dl className="grid gap-2 text-sm sm:grid-cols-2">
      <div>
        <dt className="text-muted-foreground">{t("analytics.declare.quantity")}</dt>
        <dd>{lot.quantityRemaining}</dd>
      </div>
      <div>
        <dt className="text-muted-foreground">{t("analytics.declare.instrument")}</dt>
        <dd>
          {instrumentName} ({quoteCurrency})
        </dd>
      </div>
      <div>
        <dt className="text-muted-foreground">{t("analytics.declare.account")}</dt>
        <dd>{accountName}</dd>
      </div>
      <div>
        <dt className="text-muted-foreground">
          {t("analytics.declare.acquisitionTime")}
        </dt>
        <dd>{lot.acquiredAt}</dd>
      </div>
    </dl>
  );
}
