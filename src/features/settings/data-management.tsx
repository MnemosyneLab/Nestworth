import { open as openDialog, save as saveDialog } from "@tauri-apps/plugin-dialog";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { useState, type ReactNode } from "react";
import { useTranslation } from "react-i18next";

import { Button } from "@/components/ui/button";
import {
  commands,
  type BackupInspectionDto,
  type CommandError,
  type CsvImportPreviewDto,
} from "@/generated/tauri-bindings";
import {
  commandErrorFromUnknown,
  formatCommandError,
  unwrapResult,
} from "@/lib/tauri/errors";

export function DataManagement() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const [actionError, setActionError] = useState<CommandError | null>(null);
  const [selectedPath, setSelectedPath] = useState<string | null>(null);
  const [inspection, setInspection] = useState<BackupInspectionDto | null>(null);
  const [importPath, setImportPath] = useState<string | null>(null);
  const [preview, setPreview] = useState<CsvImportPreviewDto | null>(null);
  const [overwriteConfirmed, setOverwriteConfirmed] = useState(false);
  const [restartRequired, setRestartRequired] = useState(false);

  const recovery = useQuery({
    queryKey: ["recovery-backups"],
    queryFn: () => unwrapResult(commands.listRecoveryBackups()),
  });
  const imports = useQuery({
    queryKey: ["import-batches"],
    queryFn: () =>
      unwrapResult(commands.listImportBatches({ cursor: null, limit: 20 })),
  });

  const createBackup = useMutation({
    mutationFn: (destinationPath: string) =>
      unwrapResult(commands.createBackup({ destinationPath, overwriteConfirmed })),
    onSuccess: () => {
      setActionError(null);
      setOverwriteConfirmed(false);
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const inspect = useMutation({
    mutationFn: (sourcePath: string) =>
      unwrapResult(commands.inspectBackup({ sourcePath })),
    onSuccess: (result) => {
      setInspection(result);
      setActionError(null);
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const inspectRecovery = useMutation({
    mutationFn: (backupId: string) =>
      unwrapResult(commands.inspectRecoveryBackup({ backupId })),
    onSuccess: (result) => {
      setInspection(result);
      setSelectedPath(null);
      setActionError(null);
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const restore = useMutation({
    mutationFn: (inspectionToken: string) =>
      unwrapResult(commands.restoreBackup({ inspectionToken, confirmed: true })),
    onSuccess: (result) => {
      setActionError(null);
      if (result.restartRequired) setRestartRequired(true);
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const exportJson = useMutation({
    mutationFn: (destinationPath: string) =>
      unwrapResult(
        commands.exportCanonicalJson({ destinationPath, overwriteConfirmed }),
      ),
    onSuccess: () => {
      setActionError(null);
      setOverwriteConfirmed(false);
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const exportCsv = useMutation({
    mutationFn: (input: {
      destinationPath: string;
      dataset: "activity" | "instrument_quote" | "fx_quote" | "benchmark";
    }) => unwrapResult(commands.exportCsv({ ...input, overwriteConfirmed })),
    onSuccess: () => {
      setActionError(null);
      setOverwriteConfirmed(false);
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const previewImport = useMutation({
    mutationFn: (sourcePath: string) =>
      unwrapResult(commands.previewCsvImport({ sourcePath })),
    onSuccess: (result) => {
      setPreview(result);
      setActionError(null);
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const commitImport = useMutation({
    mutationFn: (previewToken: string) =>
      unwrapResult(commands.commitCsvImport({ previewToken, confirmed: true })),
    onSuccess: async () => {
      setActionError(null);
      setPreview(null);
      await queryClient.invalidateQueries({ queryKey: ["import-batches"] });
    },
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  async function chooseBackupDestination() {
    const path = await saveDialog({
      defaultPath: "Nestworth.nestworth-backup",
      filters: [{ name: "Nestworth Backup", extensions: ["nestworth-backup"] }],
    });
    if (typeof path !== "string") return;
    setSelectedPath(path);
    setInspection(null);
    createBackup.mutate(path);
  }
  async function chooseBackupSource() {
    const path = await openDialog({
      directory: false,
      multiple: false,
      filters: [{ name: "Nestworth Backup", extensions: ["nestworth-backup"] }],
    });
    if (typeof path !== "string") return;
    setSelectedPath(path);
    inspect.mutate(path);
  }
  async function chooseJsonDestination() {
    const path = await saveDialog({
      defaultPath: "Nestworth-export.json",
      filters: [{ name: "JSON", extensions: ["json"] }],
    });
    if (typeof path === "string") exportJson.mutate(path);
  }
  async function chooseCsvDestination(
    dataset: "activity" | "instrument_quote" | "fx_quote" | "benchmark",
  ) {
    const path = await saveDialog({
      defaultPath: `Nestworth-${dataset}.csv`,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (typeof path === "string") exportCsv.mutate({ destinationPath: path, dataset });
  }
  async function chooseImportSource() {
    const path = await openDialog({
      directory: false,
      multiple: false,
      filters: [{ name: "CSV", extensions: ["csv"] }],
    });
    if (typeof path !== "string") return;
    setImportPath(path);
    previewImport.mutate(path);
  }

  if (restartRequired) {
    return (
      <section
        aria-labelledby="data-management-title"
        className="space-y-3 border-t border-border pt-6"
      >
        <h2 className="text-lg font-medium" id="data-management-title">
          {t("settings.dataManagement.title")}
        </h2>
        <div
          className="rounded-lg border border-border bg-surface-soft p-4"
          role="status"
        >
          <p className="font-medium">{t("settings.dataManagement.restartTitle")}</p>
          <p className="mt-1 text-sm text-muted-foreground">
            {t("settings.dataManagement.restartDescription")}
          </p>
        </div>
      </section>
    );
  }

  const error =
    actionError ?? (recovery.error ? commandErrorFromUnknown(recovery.error) : null);
  const inspectionReady = inspection?.checksumValid && inspection.databaseValid;

  return (
    <section
      aria-labelledby="data-management-title"
      className="space-y-5 border-t border-border pt-6"
    >
      <div>
        <h2 className="text-lg font-medium" id="data-management-title">
          {t("settings.dataManagement.title")}
        </h2>
        <p className="mt-1 max-w-2xl text-sm text-muted-foreground">
          {t("settings.dataManagement.description")}
        </p>
      </div>
      {error ? (
        <p className="text-sm text-destructive" role="alert">
          {formatCommandError(t, error)}
        </p>
      ) : null}
      <p className="rounded-lg border border-amber-500/40 bg-amber-500/5 p-3 text-sm">
        {t("settings.dataManagement.privacyWarning")}
      </p>
      <label className="flex items-center gap-2 text-sm">
        <input
          checked={overwriteConfirmed}
          onChange={(event) => setOverwriteConfirmed(event.target.checked)}
          type="checkbox"
        />
        {t("settings.dataManagement.overwrite")}
      </label>

      <div className="grid gap-3 md:grid-cols-2">
        <ActionCard
          title={t("settings.dataManagement.backupTitle")}
          description={t("settings.dataManagement.backupDescription")}
        >
          <Button
            disabled={createBackup.isPending}
            onClick={() => void chooseBackupDestination()}
            type="button"
          >
            {createBackup.isPending
              ? t("settings.dataManagement.working")
              : t("settings.dataManagement.createBackup")}
          </Button>
        </ActionCard>
        <ActionCard
          title={t("settings.dataManagement.restoreTitle")}
          description={t("settings.dataManagement.restoreDescription")}
        >
          <Button
            disabled={inspect.isPending}
            onClick={() => void chooseBackupSource()}
            type="button"
            variant="ghost"
          >
            {t("settings.dataManagement.inspectBackup")}
          </Button>
          {selectedPath ? (
            <p className="mt-2 break-all text-xs text-muted-foreground">
              {selectedPath}
            </p>
          ) : null}
        </ActionCard>
      </div>
      {inspection ? (
        <InspectionPanel
          inspection={inspection}
          onRestore={() => restore.mutate(inspection.inspectionToken)}
          restoring={restore.isPending}
          ready={Boolean(inspectionReady)}
          t={t}
        />
      ) : null}

      <ActionCard
        title={t("settings.dataManagement.exportTitle")}
        description={t("settings.dataManagement.exportDescription")}
      >
        <div className="flex flex-wrap gap-2">
          <Button
            disabled={exportJson.isPending}
            onClick={() => void chooseJsonDestination()}
            type="button"
            variant="ghost"
          >
            {t("settings.dataManagement.exportJson")}
          </Button>
          <Button
            disabled={exportCsv.isPending}
            onClick={() => void chooseCsvDestination("activity")}
            type="button"
            variant="ghost"
          >
            {t("settings.dataManagement.exportActivityCsv")}
          </Button>
          <Button
            disabled={exportCsv.isPending}
            onClick={() => void chooseCsvDestination("instrument_quote")}
            type="button"
            variant="ghost"
          >
            {t("settings.dataManagement.exportInstrumentQuoteCsv")}
          </Button>
          <Button
            disabled={exportCsv.isPending}
            onClick={() => void chooseCsvDestination("fx_quote")}
            type="button"
            variant="ghost"
          >
            {t("settings.dataManagement.exportFxQuoteCsv")}
          </Button>
          <Button
            disabled={exportCsv.isPending}
            onClick={() => void chooseCsvDestination("benchmark")}
            type="button"
            variant="ghost"
          >
            {t("settings.dataManagement.exportBenchmarkCsv")}
          </Button>
        </div>
      </ActionCard>

      <ActionCard
        title={t("settings.dataManagement.importTitle")}
        description={t("settings.dataManagement.importDescription")}
      >
        <Button
          disabled={previewImport.isPending}
          onClick={() => void chooseImportSource()}
          type="button"
          variant="ghost"
        >
          {t("settings.dataManagement.chooseCsv")}
        </Button>
        {importPath ? (
          <p className="mt-2 break-all text-xs text-muted-foreground">{importPath}</p>
        ) : null}
        {preview ? (
          <ImportPreview
            preview={preview}
            onCommit={() => commitImport.mutate(preview.previewToken)}
            committing={commitImport.isPending}
            t={t}
          />
        ) : null}
      </ActionCard>

      <div>
        <h3 className="font-medium">{t("settings.dataManagement.recoveryTitle")}</h3>
        <p className="mt-1 text-sm text-muted-foreground">
          {recovery.data?.explanation ??
            t("settings.dataManagement.recoveryDescription")}
        </p>
        {recovery.isPending ? (
          <p className="mt-2 text-sm" role="status">
            {t("references.loading")}
          </p>
        ) : null}
        <div className="mt-3 space-y-2">
          {(recovery.data?.items ?? []).map((item) => (
            <div
              className="flex flex-wrap items-center justify-between gap-2 rounded-lg border border-border p-3"
              key={item.id}
            >
              <span className="text-sm">
                {item.householdName} · {item.createdAt}
              </span>
              <Button
                onClick={() => inspectRecovery.mutate(item.id)}
                type="button"
                variant="ghost"
              >
                {t("settings.dataManagement.inspectRecovery")}
              </Button>
            </div>
          ))}
        </div>
      </div>
      <div>
        <h3 className="font-medium">{t("settings.dataManagement.importHistory")}</h3>
        <div className="mt-2 space-y-2">
          {(imports.data?.items ?? []).map((item) => (
            <div className="rounded-lg border border-border p-3 text-sm" key={item.id}>
              <div className="flex justify-between gap-2">
                <span>{item.template}</span>
                <span>{item.status}</span>
              </div>
              <p className="mt-1 text-xs text-muted-foreground">
                {item.rowCount} rows · {item.committedCount} committed ·{" "}
                {item.duplicateCount} duplicates
              </p>
            </div>
          ))}
        </div>
      </div>
    </section>
  );
}

function ActionCard({
  title,
  description,
  children,
}: {
  title: string;
  description: string;
  children: ReactNode;
}) {
  return (
    <div className="rounded-lg border border-border bg-card p-4">
      <h3 className="font-medium">{title}</h3>
      <p className="mt-1 mb-3 text-sm text-muted-foreground">{description}</p>
      {children}
    </div>
  );
}

function InspectionPanel({
  inspection,
  onRestore,
  restoring,
  ready,
  t,
}: {
  inspection: BackupInspectionDto;
  onRestore: () => void;
  restoring: boolean;
  ready: boolean;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  return (
    <div className="rounded-lg border border-border bg-surface-soft p-4">
      <h3 className="font-medium">{t("settings.dataManagement.inspectionTitle")}</h3>
      <dl className="mt-3 grid gap-2 text-sm sm:grid-cols-2">
        <div>
          <dt className="text-muted-foreground">
            {t("settings.dataManagement.household")}
          </dt>
          <dd>{inspection.manifest.householdName}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">
            {t("settings.dataManagement.migration")}
          </dt>
          <dd>{inspection.manifest.databaseMigrationVersion}</dd>
        </div>
        <div>
          <dt className="text-muted-foreground">
            {t("settings.dataManagement.checksum")}
          </dt>
          <dd>
            {inspection.checksumValid
              ? t("settings.dataManagement.valid")
              : t("settings.dataManagement.invalid")}
          </dd>
        </div>
        <div>
          <dt className="text-muted-foreground">
            {t("settings.dataManagement.encryption")}
          </dt>
          <dd>
            {inspection.encrypted
              ? t("settings.dataManagement.encrypted")
              : t("settings.dataManagement.notEncrypted")}
          </dd>
        </div>
      </dl>
      {ready ? (
        <div className="mt-4">
          <p className="mb-2 text-sm">{t("settings.dataManagement.restoreConfirm")}</p>
          <Button
            disabled={restoring}
            onClick={onRestore}
            type="button"
            variant="destructive"
          >
            {restoring
              ? t("settings.dataManagement.working")
              : t("settings.dataManagement.restore")}
          </Button>
        </div>
      ) : (
        <p className="mt-3 text-sm text-destructive">
          {t("settings.dataManagement.inspectionInvalid")}
        </p>
      )}
    </div>
  );
}

function ImportPreview({
  preview,
  onCommit,
  committing,
  t,
}: {
  preview: CsvImportPreviewDto;
  onCommit: () => void;
  committing: boolean;
  t: (key: string, options?: Record<string, unknown>) => string;
}) {
  return (
    <div className="mt-4 rounded-lg border border-border bg-surface-soft p-4">
      <h3 className="font-medium">{t("settings.dataManagement.previewTitle")}</h3>
      <p className="mt-1 text-sm">
        {preview.template} · {preview.rowCount} rows · {preview.validCount} valid ·{" "}
        {preview.duplicateCount} duplicates · {preview.errorCount} errors
      </p>
      {preview.diagnostics.length > 0 ? (
        <ul
          className="mt-3 max-h-40 overflow-auto text-sm"
          aria-label={t("settings.dataManagement.diagnostics")}
        >
          {preview.diagnostics.map((diagnostic) => (
            <li
              className="border-t border-border py-1"
              key={`${diagnostic.row}-${diagnostic.field}-${diagnostic.code}`}
            >
              {t("settings.dataManagement.diagnostic", diagnostic)}
            </li>
          ))}
        </ul>
      ) : null}
      <Button
        className="mt-3"
        disabled={!preview.canCommit || committing}
        onClick={onCommit}
        type="button"
      >
        {committing
          ? t("settings.dataManagement.working")
          : t("settings.dataManagement.commitImport")}
      </Button>
    </div>
  );
}
