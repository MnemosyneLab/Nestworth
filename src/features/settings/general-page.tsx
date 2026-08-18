import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type ComponentPropsWithoutRef } from "react";
import { useTranslation } from "react-i18next";

import { AppShell } from "@/components/app-shell";
import { Button } from "@/components/ui/button";
import { commands, type CommandError } from "@/generated/tauri-bindings";
import { i18n } from "@/lib/i18n";
import { applyAppearance, resolveLanguage } from "@/lib/i18n/preferences";
import {
  referenceCatalogFromBootstrap,
  referenceCurrencyCodeLabel,
} from "@/lib/reference-catalog";
import { bootstrapQueryKey, useBootstrapQuery } from "@/lib/tauri/bootstrap";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";
import { cn } from "@/lib/utils";

export function SettingsGeneralPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const bootstrap = useBootstrapQuery();
  const household =
    bootstrap.data?.status === "ready" ? bootstrap.data.household : null;
  const catalog = referenceCatalogFromBootstrap(bootstrap.data);
  const [actionError, setActionError] = useState<CommandError | null>(null);
  const [confirmingDelete, setConfirmingDelete] = useState(false);

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

  const deleteData = useMutation({
    mutationFn: () => unwrapResult(commands.deleteAllData({ confirmed: true })),
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
                  {t("overview.baseCurrency", {
                    currency: referenceCurrencyCodeLabel(
                      t,
                      catalog,
                      household.baseCurrency,
                    ),
                  })}
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
                {catalog.languages.map((value) => (
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
                {catalog.appearances.map((value) => (
                  <option key={value} value={value}>
                    {t(`settings.appearances.${value}`)}
                  </option>
                ))}
              </NativeSelect>
            </label>
            <section
              aria-labelledby="delete-all-data-title"
              className="space-y-3 border-t border-border pt-6"
            >
              <div className="space-y-1">
                <h2 className="text-lg font-medium" id="delete-all-data-title">
                  {t("settings.deleteAllData.title")}
                </h2>
                <p className="max-w-xl text-sm text-muted-foreground">
                  {t("settings.deleteAllData.description")}
                </p>
              </div>
              {confirmingDelete ? (
                <div
                  aria-labelledby="delete-all-data-confirm-title"
                  className="max-w-xl space-y-3 rounded-lg border border-destructive/40 bg-destructive/5 p-4"
                  role="group"
                >
                  <p className="font-medium" id="delete-all-data-confirm-title">
                    {t("settings.deleteAllData.confirmTitle")}
                  </p>
                  <p className="text-sm text-muted-foreground">
                    {t("settings.deleteAllData.confirmDescription")}
                  </p>
                  <div className="flex flex-wrap gap-2">
                    <Button
                      autoFocus
                      disabled={deleteData.isPending}
                      onClick={() => {
                        setActionError(null);
                        deleteData.mutate();
                      }}
                      type="button"
                      variant="destructive"
                    >
                      {deleteData.isPending
                        ? t("settings.deleteAllData.deleting")
                        : t("settings.deleteAllData.confirm")}
                    </Button>
                    <Button
                      disabled={deleteData.isPending}
                      onClick={() => setConfirmingDelete(false)}
                      type="button"
                      variant="ghost"
                    >
                      {t("references.cancel")}
                    </Button>
                  </div>
                </div>
              ) : (
                <Button
                  onClick={() => {
                    setActionError(null);
                    setConfirmingDelete(true);
                  }}
                  type="button"
                  variant="destructive"
                >
                  {t("settings.deleteAllData.action")}
                </Button>
              )}
            </section>
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
        "h-10 w-full rounded-lg border border-border bg-card px-3 text-sm font-normal text-foreground shadow-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring",
        className,
      )}
    />
  );
}

export default SettingsGeneralPage;
