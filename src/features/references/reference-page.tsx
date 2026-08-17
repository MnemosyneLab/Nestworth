import type { ComponentPropsWithoutRef, ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { AppShell } from "@/components/app-shell";
import { Button } from "@/components/ui/button";
import type { CommandError } from "@/generated/tauri-bindings";
import { formatCommandError } from "@/lib/tauri/errors";
import { cn } from "@/lib/utils";

export function ReferencePage({
  title,
  addLabel,
  onAdd,
  showArchived,
  onShowArchivedChange,
  loading,
  error,
  empty,
  isEmpty,
  children,
}: {
  title: string;
  addLabel: string;
  onAdd: () => void;
  showArchived: boolean;
  onShowArchivedChange: (value: boolean) => void;
  loading: boolean;
  error: CommandError | null;
  empty: string;
  isEmpty: boolean;
  children: ReactNode;
}) {
  const { t } = useTranslation();

  return (
    <AppShell>
      <main className="mx-auto max-w-3xl px-8 py-10">
        <div className="mb-6 flex flex-wrap items-center justify-between gap-4">
          <h1 className="text-3xl font-semibold tracking-tight">{title}</h1>
          <Button onClick={onAdd} type="button">
            {addLabel}
          </Button>
        </div>
        <label className="mb-6 flex items-center gap-2 text-sm text-muted-foreground">
          <input
            checked={showArchived}
            onChange={(event) => onShowArchivedChange(event.target.checked)}
            type="checkbox"
          />
          {t("references.showArchived")}
        </label>
        {error ? (
          <p className="mb-6 text-sm text-destructive" role="alert">
            {formatCommandError(t, error)}
          </p>
        ) : null}
        {loading ? (
          <p role="status">{t("references.loading")}</p>
        ) : isEmpty ? (
          <p className="text-muted-foreground">{empty}</p>
        ) : (
          children
        )}
      </main>
    </AppShell>
  );
}

export function RecordCard({
  name,
  archived,
  details,
  leading,
  children,
}: {
  name: string;
  archived: boolean;
  details?: ReactNode;
  leading?: ReactNode;
  children: ReactNode;
}) {
  const { t } = useTranslation();
  return (
    <article
      className={cn(
        "rounded-xl border border-muted bg-card px-4 py-4 shadow-sm",
        archived && "opacity-70",
      )}
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 items-start gap-3">
          {leading}
          <div>
            <h2 className="text-lg font-medium">{name}</h2>
            {archived ? (
              <p className="mt-1 text-xs uppercase tracking-wide text-muted-foreground">
                {t("references.archived")}
              </p>
            ) : null}
            {details}
          </div>
        </div>
        <div className="flex flex-wrap gap-2">{children}</div>
      </div>
    </article>
  );
}

export function GhostButton({
  children,
  ...props
}: ComponentPropsWithoutRef<typeof Button>) {
  return (
    <Button
      {...props}
      className="h-9 bg-transparent px-3 text-foreground shadow-none hover:bg-muted"
    >
      {children}
    </Button>
  );
}
