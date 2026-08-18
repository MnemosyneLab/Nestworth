import { useMutation, useQueryClient } from "@tanstack/react-query";
import { useNavigate } from "@tanstack/react-router";
import { useEffect, useId, useRef, useState, type FormEvent } from "react";
import { useFieldArray, useForm, type Path } from "react-hook-form";
import { useTranslation } from "react-i18next";

import { Brand } from "@/components/brand";
import { Button } from "@/components/ui/button";
import { Input } from "@/components/ui/input";
import {
  defaultOnboardingValues,
  onboardingSchema,
  parseOnboardingStep,
  type OnboardingFormValues,
} from "@/features/onboarding/schema";
import { commands, type CommandError } from "@/generated/tauri-bindings";
import {
  groupReferenceOptions,
  referenceCatalogFromBootstrap,
  referenceGroupLabel,
  referenceOptionLabel,
} from "@/lib/reference-catalog";
import { bootstrapQueryKey, useBootstrapQuery } from "@/lib/tauri/bootstrap";
import { commandErrorFromUnknown } from "@/lib/tauri/errors";

const LAST_STEP = 3;

export function OnboardingPage() {
  const { t } = useTranslation();
  const navigate = useNavigate();
  const queryClient = useQueryClient();
  const bootstrap = useBootstrapQuery();
  const catalog = referenceCatalogFromBootstrap(bootstrap.data);
  const formId = useId();
  const errorSummaryRef = useRef<HTMLDivElement>(null);
  const submitRef = useRef<HTMLButtonElement>(null);
  const [step, setStep] = useState(0);
  const [serverError, setServerError] = useState<CommandError | null>(null);

  const form = useForm<OnboardingFormValues>({
    defaultValues: defaultOnboardingValues,
    mode: "onSubmit",
  });
  const members = useFieldArray({ control: form.control, name: "members" });
  const submittingLock = useRef(false);

  const mutation = useMutation({
    mutationFn: async (values: OnboardingFormValues) => {
      const result = await commands.completeOnboarding({
        householdName: values.householdName.trim(),
        baseCurrency: values.baseCurrency.trim(),
        members: values.members.map((member) => ({ name: member.name.trim() })),
      });
      if (result.status === "error") {
        throw result.error;
      }
    },
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: bootstrapQueryKey });
      await queryClient.refetchQueries({ queryKey: bootstrapQueryKey });
      await navigate({ to: "/overview", replace: true });
    },
    onError: (error) => {
      const commandError = commandErrorFromUnknown(error);
      setServerError(commandError);
      applyServerFieldErrors(form, commandError);
      window.setTimeout(() => {
        const firstField = commandError.fields
          ? Object.keys(commandError.fields)[0]
          : undefined;
        if (firstField && firstField !== "members") {
          form.setFocus(firstField as Path<OnboardingFormValues>);
          return;
        }
        errorSummaryRef.current?.focus();
      }, 0);
    },
  });

  useEffect(() => {
    if (step === 0) {
      form.setFocus("householdName");
    }
    if (step === 1) {
      form.setFocus("baseCurrency");
    }
    if (step === 2) {
      form.setFocus("members.0.name");
    }
    if (step === 3) {
      submitRef.current?.focus();
    }
  }, [form, step]);

  const busy = mutation.isPending || submittingLock.current;

  async function goNext() {
    const parsed = parseOnboardingStep(step, form.getValues());
    if (!parsed.success) {
      applyZodIssues(form, parsed.error.issues);
      const first = parsed.error.issues[0]?.path.join(".");
      if (first) {
        form.setFocus(first as Path<OnboardingFormValues>);
      }
      return;
    }
    setServerError(null);
    setStep((current) => current + 1);
  }

  async function onSubmit(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (busy) {
      return;
    }
    if (step < LAST_STEP) {
      await goNext();
      return;
    }

    const parsed = onboardingSchema.safeParse(form.getValues());
    if (!parsed.success) {
      applyZodIssues(form, parsed.error.issues);
      const first = parsed.error.issues[0]?.path.join(".");
      if (first) {
        form.setFocus(first as Path<OnboardingFormValues>);
      }
      return;
    }

    submittingLock.current = true;
    setServerError(null);
    try {
      await mutation.mutateAsync(parsed.data);
    } catch {
      // Field and summary errors are applied in mutation.onError.
    } finally {
      submittingLock.current = false;
    }
  }

  return (
    <main className="mx-auto flex min-h-screen max-w-xl flex-col justify-center px-8 py-16">
      <Brand className="mb-8" size="lg" />
      <p className="mb-3 text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
        {t("onboarding.eyebrow")}
      </p>
      <h1 className="text-4xl font-semibold tracking-tight">{t("onboarding.title")}</h1>
      <p className="mt-3 text-muted-foreground" id={`${formId}-step`}>
        {t("onboarding.step", { current: step + 1, total: 4 })}
      </p>

      {serverError ? (
        <div
          ref={errorSummaryRef}
          className="mt-6 rounded-lg border border-destructive/30 bg-card px-4 py-3 text-sm text-destructive"
          role="alert"
          tabIndex={-1}
        >
          {t(`errors.${serverError.code}`, { defaultValue: serverError.message })}
        </div>
      ) : null}

      <form
        aria-describedby={`${formId}-step`}
        className="mt-8 space-y-6"
        noValidate
        onSubmit={onSubmit}
      >
        {step === 0 ? (
          <div className="space-y-2">
            <label className="text-sm font-medium" htmlFor={`${formId}-household`}>
              {t("onboarding.householdName")}
            </label>
            <Input
              aria-invalid={form.formState.errors.householdName ? true : undefined}
              autoComplete="organization"
              id={`${formId}-household`}
              {...form.register("householdName")}
            />
            <FieldError
              message={translateFieldError(
                t,
                form.formState.errors.householdName?.message,
              )}
            />
          </div>
        ) : null}

        {step === 1 ? (
          <fieldset className="space-y-3">
            <legend className="text-sm font-medium">
              {t("onboarding.baseCurrency")}
            </legend>
            <select
              aria-invalid={form.formState.errors.baseCurrency ? true : undefined}
              className="h-10 w-full rounded-lg border border-border bg-card px-3 text-sm text-foreground outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring"
              id={`${formId}-currency`}
              {...form.register("baseCurrency")}
            >
              {groupReferenceOptions(catalog.currencies).map(([group, options]) => (
                <optgroup
                  key={group}
                  label={referenceGroupLabel(t, "currencyGroups", group)}
                >
                  {options.map((option) => (
                    <option key={option.value} value={option.value}>
                      {referenceOptionLabel(t, "currencies", option.value)}
                    </option>
                  ))}
                </optgroup>
              ))}
            </select>
            <FieldError
              message={translateFieldError(
                t,
                form.formState.errors.baseCurrency?.message,
              )}
            />
          </fieldset>
        ) : null}

        {step === 2 ? (
          <div className="space-y-4">
            <div className="flex items-center justify-between gap-3">
              <h2 className="text-lg font-medium">{t("onboarding.members")}</h2>
              <Button
                onClick={() => members.append({ name: "" })}
                type="button"
                variant="ghost"
              >
                {t("onboarding.addMember")}
              </Button>
            </div>
            {members.fields.map((field, index) => (
              <div className="space-y-2" key={field.id}>
                <label
                  className="text-sm font-medium"
                  htmlFor={`${formId}-member-${index}`}
                >
                  {t("onboarding.memberName", { number: index + 1 })}
                </label>
                <div className="flex gap-2">
                  <Input
                    aria-invalid={
                      form.formState.errors.members?.[index]?.name ? true : undefined
                    }
                    id={`${formId}-member-${index}`}
                    {...form.register(`members.${index}.name`)}
                  />
                  <Button
                    aria-label={t("onboarding.removeMember", { number: index + 1 })}
                    disabled={members.fields.length === 1}
                    onClick={() => members.remove(index)}
                    type="button"
                    variant="ghost"
                  >
                    {t("onboarding.remove")}
                  </Button>
                </div>
                <FieldError
                  message={translateFieldError(
                    t,
                    form.formState.errors.members?.[index]?.name?.message,
                  )}
                />
              </div>
            ))}
          </div>
        ) : null}

        {step === 3 ? (
          <section
            className="space-y-4 rounded-2xl bg-card px-5 py-4 shadow-sm"
            aria-labelledby={`${formId}-review`}
          >
            <h2 className="text-lg font-medium" id={`${formId}-review`}>
              {t("onboarding.review")}
            </h2>
            <p>
              <span className="text-muted-foreground">
                {t("onboarding.householdName")}:{" "}
              </span>
              {form.getValues("householdName")}
            </p>
            <p>
              <span className="text-muted-foreground">
                {t("onboarding.baseCurrency")}:{" "}
              </span>
              {referenceOptionLabel(t, "currencies", form.getValues("baseCurrency"))}
            </p>
            <ul className="list-disc pl-5">
              {form.getValues("members").map((member, index) => (
                <li key={`${member.name}-${index}`}>{member.name}</li>
              ))}
            </ul>
          </section>
        ) : null}

        <div className="flex gap-3">
          {step > 0 ? (
            <Button
              onClick={() => setStep((current) => current - 1)}
              type="button"
              variant="ghost"
            >
              {t("onboarding.back")}
            </Button>
          ) : null}
          <Button disabled={busy} ref={submitRef} type="submit">
            {step === LAST_STEP
              ? busy
                ? t("onboarding.saving")
                : t("onboarding.finish")
              : t("onboarding.next")}
          </Button>
        </div>
      </form>
    </main>
  );
}

function FieldError({ message }: { message?: string }) {
  if (!message) {
    return null;
  }
  return (
    <p className="text-sm text-destructive" role="alert">
      {message}
    </p>
  );
}

function translateFieldError(t: (key: string) => string, message: string | undefined) {
  if (!message) {
    return undefined;
  }
  if (
    message === "required" ||
    message === "tooLong" ||
    message === "currency" ||
    message === "members"
  ) {
    return t(`onboarding.errors.${message}`);
  }
  return message;
}

function applyZodIssues(
  form: ReturnType<typeof useForm<OnboardingFormValues>>,
  issues: Array<{ path: PropertyKey[]; message: string }>,
) {
  for (const issue of issues) {
    const name = issue.path.join(".");
    if (name) {
      form.setError(name as Path<OnboardingFormValues>, {
        type: "zod",
        message: issue.message,
      });
    }
  }
}

function applyServerFieldErrors(
  form: ReturnType<typeof useForm<OnboardingFormValues>>,
  error: CommandError,
) {
  if (!error.fields) {
    return;
  }
  for (const [field, message] of Object.entries(error.fields)) {
    if (!message) {
      continue;
    }
    form.setError(field as Path<OnboardingFormValues>, {
      type: "server",
      message,
    });
  }
}

export default OnboardingPage;
