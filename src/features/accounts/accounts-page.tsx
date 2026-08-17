import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { useState } from "react";
import { useTranslation } from "react-i18next";

import { AccountForm } from "@/features/accounts/account-form";
import { formatMoney } from "@/features/accounts/schema";
import {
  GhostButton,
  RecordCard,
  ReferencePage,
} from "@/features/references/reference-page";
import {
  commands,
  type AccountRecordDto,
  type CommandError,
} from "@/generated/tauri-bindings";
import { bootstrapQueryKey, useBootstrapQuery } from "@/lib/tauri/bootstrap";
import { commandErrorFromUnknown, unwrapResult } from "@/lib/tauri/errors";

export function AccountsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const bootstrap = useBootstrapQuery();
  const household =
    bootstrap.data?.status === "ready" ? bootstrap.data.household : null;
  const members = bootstrap.data?.status === "ready" ? bootstrap.data.members : [];
  const [showArchived, setShowArchived] = useState(false);
  const [creating, setCreating] = useState(false);
  const [actionError, setActionError] = useState<CommandError | null>(null);

  const list = useQuery({
    queryKey: ["accounts", showArchived],
    queryFn: () =>
      unwrapResult(commands.listAccounts({ includeArchived: showArchived })),
  });
  const institutions = useQuery({
    queryKey: ["institutions", true],
    queryFn: () => unwrapResult(commands.listInstitutions({ includeArchived: true })),
  });
  const groups = useQuery({
    queryKey: ["groups", true],
    queryFn: () => unwrapResult(commands.listGroups({ includeArchived: true })),
  });

  async function invalidate() {
    await queryClient.invalidateQueries({ queryKey: ["accounts"] });
    await queryClient.invalidateQueries({ queryKey: bootstrapQueryKey });
  }

  const archive = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.archiveAccount({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });
  const restore = useMutation({
    mutationFn: (id: string) => unwrapResult(commands.restoreAccount({ id })),
    onSuccess: invalidate,
    onError: (error) => setActionError(commandErrorFromUnknown(error)),
  });

  const items = list.data ?? [];
  const listError = list.error ? commandErrorFromUnknown(list.error) : actionError;

  return (
    <ReferencePage
      addLabel={t("accounts.add")}
      empty={t("accounts.empty")}
      error={listError}
      isEmpty={!creating && items.length === 0}
      loading={list.isPending}
      onAdd={() => {
        setActionError(null);
        setCreating(true);
      }}
      onShowArchivedChange={setShowArchived}
      showArchived={showArchived}
      title={t("accounts.title")}
    >
      <div className="space-y-3">
        {creating && household ? (
          <AccountForm
            defaultCurrency={household.baseCurrency}
            groups={groups.data ?? []}
            institutions={institutions.data ?? []}
            members={members}
            onCancel={() => setCreating(false)}
            onSaved={async () => {
              setCreating(false);
              await invalidate();
            }}
          />
        ) : null}
        {items.map((account) => (
          <RecordCard
            archived={Boolean(account.archivedAt)}
            details={<AccountSummary account={account} />}
            key={account.id}
            name={account.name}
          >
            <Link
              className="inline-flex h-9 items-center rounded-lg px-3 text-sm hover:bg-muted"
              params={{ accountId: account.id }}
              to="/accounts/$accountId"
            >
              {t("accounts.open")}
            </Link>
            {account.archivedAt ? (
              <GhostButton
                aria-label={t("accounts.restoreName", { name: account.name })}
                disabled={restore.isPending}
                onClick={() => restore.mutate(account.id)}
                type="button"
              >
                {t("references.restore")}
              </GhostButton>
            ) : (
              <GhostButton
                aria-label={t("accounts.archiveName", { name: account.name })}
                disabled={archive.isPending}
                onClick={() => archive.mutate(account.id)}
                type="button"
              >
                {t("references.archive")}
              </GhostButton>
            )}
          </RecordCard>
        ))}
      </div>
    </ReferencePage>
  );
}

function AccountSummary({ account }: { account: AccountRecordDto }) {
  const { t } = useTranslation();
  const value = account.latestValue
    ? formatMoney(account.latestValue.amount, account.latestValue.currency)
    : t("accounts.noValue");
  const owners = account.owners.map((owner) => owner.memberName).join(", ");
  return (
    <div className="mt-1 space-y-1 text-sm text-muted-foreground">
      <p>{value}</p>
      <p>{t(`accounts.categories.${account.secondaryCategory}`)}</p>
      {owners ? <p>{owners}</p> : null}
    </div>
  );
}
