import { useTranslation } from "react-i18next";

import type { BootstrapDto, CommandError } from "@/generated/tauri-bindings";
import { commandErrorFromUnknown } from "@/lib/tauri/errors";

export function StartupErrorPage({
  bootstrap,
  error,
}: {
  bootstrap?: BootstrapDto;
  error?: unknown;
}) {
  const { t } = useTranslation();
  const blocked = bootstrap?.status === "blocked" ? bootstrap : null;
  const commandError: CommandError = blocked?.error ?? commandErrorFromUnknown(error);
  const foundMigration = blocked?.foundMigration ?? commandError.fields?.foundMigration;
  const supportedMigration =
    blocked?.supportedMigration ?? commandError.fields?.supportedMigration;

  return (
    <main className="mx-auto flex min-h-screen max-w-xl flex-col justify-center px-8 py-16">
      <p className="mb-3 text-sm font-medium uppercase tracking-[0.2em] text-muted-foreground">
        {t("startup.eyebrow")}
      </p>
      <h1 className="text-4xl font-semibold tracking-tight">{t("startup.title")}</h1>
      <p className="mt-4 text-lg text-muted-foreground" role="alert">
        {t(`errors.${commandError.code}`, { defaultValue: commandError.message })}
      </p>
      {commandError.code === "UNSUPPORTED_NEWER_DATABASE" ? (
        <p className="mt-3 text-muted-foreground">
          {t("startup.unsupportedVersion", {
            found: foundMigration ?? "?",
            supported: supportedMigration ?? "?",
          })}
        </p>
      ) : null}
      {blocked?.databasePath ? (
        <p className="mt-6 break-all text-sm text-muted-foreground">
          {t("startup.databasePath", { path: blocked.databasePath })}
        </p>
      ) : null}
    </main>
  );
}
