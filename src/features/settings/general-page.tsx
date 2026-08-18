import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type ComponentPropsWithoutRef } from "react";
import { useTranslation } from "react-i18next";

import { AppShell } from "@/components/app-shell";
import { commands, type CommandError } from "@/generated/tauri-bindings";
import { i18n } from "@/lib/i18n";
import { applyAppearance, resolveLanguage } from "@/lib/i18n/preferences";
import { bootstrapQueryKey, useBootstrapQuery } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { cn } from "@/lib/utils";

const LANGUAGES = ["system", "en", "zh-CN"] as const;
const APPEARANCES = ["system", "light", "dark"] as const;

export function SettingsGeneralPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const bootstrap = useBootstrapQuery();
  const household =
    bootstrap.data?.status === "ready" ? bootstrap.data.household : null;
  const [actionError, setActionError] = useState<CommandError | null>(null);

  const settings = useQuery({
    queryKey: ["settings"],
    queryFn: () => unwrapResult(commands.getSettings()),
  });

  const update = useMutation({
    mutationFn: (input: { language: string; appearance: string }) =>
      unwrapResult(commands.updateSettings(input)),
    onSuccess: async (next) => {
      queryClient.setQueryData(["settings"], next);
      await i18n.changeLanguage(resolveLanguage(next.language));
      applyAppearance(next.appearance);
      await queryClient.invalidateQueries({ queryKey: bootstrapQueryKey });
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  const error = settings.error ? commandErrorFromUnknown(settings.error) : actionError;

  return (
    <AppShell>
      <main className="mx-auto max-w-3xl px-8 py-10">
        <h1 className="text-3xl font-semibold tracking-tight">{t("settings.title")}</h1>
        {error ? (
          <p className="mt-6 text-sm text-destructive" role="alert">
            {formatCommandError(t, error)}
          </p>
        ) : null}
        {settings.isPending ? (
          <p className="mt-6" role="status">
            {t("references.loading")}
          </p>
        ) : null}
        {settings.data ? (
          <div className="mt-8 space-y-6">
            {household ? (
              <section className="space-y-2">
                <h2 className="text-lg font-medium">{t("settings.household")}</h2>
                <p className="text-sm text-muted-foreground">{household.name}</p>
                <p className="text-sm text-muted-foreground">
                  {t("overview.baseCurrency", { currency: household.baseCurrency })}
                </p>
              </section>
            ) : null}
            <label className="grid max-w-sm gap-2 text-sm font-medium">
              {t("settings.language")}
              <NativeSelect
                aria-label={t("settings.language")}
                onChange={(event) => {
                  setActionError(null);
                  update.mutate({
                    language: event.target.value,
                    appearance: settings.data.appearance,
                  });
                }}
                value={settings.data.language}
              >
                {LANGUAGES.map((value) => (
                  <option key={value} value={value}>
                    {t(`settings.languages.${value}`)}
                  </option>
                ))}
              </NativeSelect>
            </label>
            <label className="grid max-w-sm gap-2 text-sm font-medium">
              {t("settings.appearance")}
              <NativeSelect
                aria-label={t("settings.appearance")}
                onChange={(event) => {
                  setActionError(null);
                  update.mutate({
                    language: settings.data.language,
                    appearance: event.target.value,
                  });
                }}
                value={settings.data.appearance}
              >
                {APPEARANCES.map((value) => (
                  <option key={value} value={value}>
                    {t(`settings.appearances.${value}`)}
                  </option>
                ))}
              </NativeSelect>
            </label>
          </div>
        ) : null}
      </main>
    </AppShell>
  );
}

function NativeSelect({ className, ...props }: ComponentPropsWithoutRef<"select">) {
  return (
    <select
      {...props}
      className={cn(
        "h-10 w-full rounded-lg border border-muted bg-card px-3 text-sm font-normal text-foreground shadow-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring",
        className,
      )}
    />
  );
}

export default SettingsGeneralPage;
