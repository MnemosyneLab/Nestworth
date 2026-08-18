import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { getRouteApi, Link } from "@tanstack/react-router";
import { useState, type ComponentPropsWithoutRef } from "react";
import { useTranslation } from "react-i18next";

import { AccountForm } from "@/features/accounts/account-form";
import { AccountMark } from "@/features/accounts/account-mark";
import { formatMoney, PRIMARY_CATEGORIES } from "@/features/accounts/schema";
import {
  accountMatchesSearch,
  mergeAccountSearch,
  type AccountSearch,
} from "@/features/accounts/search";
import {
  GhostButton,
  RecordCard,
  ReferencePage,
} from "@/features/references/reference-page";
import {
  commands,
  type AccountRecordDto,
  type CommandError,
  type GroupRecordDto,
  type InstitutionRecordDto,
} from "@/generated/tauri-bindings";
import { invalidateValuation } from "@/lib/tauri/invalidate";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";
import { commandErrorFromUnknown, unwrapResult } from "@/lib/tauri/errors";
import { cn } from "@/lib/utils";

const accountsRoute = getRouteApi("/accounts");

export function AccountsPage() {
  const { t } = useTranslation();
  const queryClient = useQueryClient();
  const search = accountsRoute.useSearch();
  const navigate = accountsRoute.useNavigate();
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
    await invalidateValuation(queryClient);
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
  const visible = items.filter((account) => accountMatchesSearch(account, search));
  const listError = list.error ? commandErrorFromUnknown(list.error) : actionError;

  function patchSearch(patch: Partial<AccountSearch>) {
    navigate({
      search: (prev) => mergeAccountSearch(prev, patch),
    });
  }

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
        {items.length > 0 ? (
          <AccountFilters
            groups={groups.data ?? []}
            institutions={institutions.data ?? []}
            onPatch={patchSearch}
            search={search}
          />
        ) : null}
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
        {visible.length === 0 && !creating ? (
          <p className="text-muted-foreground">{t("accounts.filterEmpty")}</p>
        ) : null}
        {visible.map((account) => (
          <RecordCard
            archived={Boolean(account.archivedAt)}
            details={<AccountSummary account={account} />}
            key={account.id}
            leading={
              <AccountMark account={account} institutions={institutions.data ?? []} />
            }
            name={account.name}
          >
            <Link
              className="inline-flex h-9 items-center rounded-lg px-3 text-sm hover:bg-surface-soft"
              params={{ accountId: account.id }}
              search={(prev) => prev}
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

function AccountFilters({
  groups,
  institutions,
  onPatch,
  search,
}: {
  groups: GroupRecordDto[];
  institutions: InstitutionRecordDto[];
  onPatch: (patch: Partial<AccountSearch>) => void;
  search: AccountSearch;
}) {
  const { t } = useTranslation();
  return (
    <div
      aria-label={t("accounts.filters")}
      className="mb-3 grid gap-3 sm:grid-cols-3"
      role="search"
    >
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("accounts.category")}
        <NativeSelect
          onChange={(event) => onPatch({ category: event.target.value || undefined })}
          value={search.category ?? ""}
        >
          <option value="">{t("accounts.allCategories")}</option>
          {PRIMARY_CATEGORIES.map((category) => (
            <option key={category} value={category}>
              {t(`accounts.primaries.${category}`)}
            </option>
          ))}
        </NativeSelect>
      </label>
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("accounts.institution")}
        <NativeSelect
          onChange={(event) =>
            onPatch({ institution: event.target.value || undefined })
          }
          value={search.institution ?? ""}
        >
          <option value="">{t("accounts.allInstitutions")}</option>
          {filterChoices(institutions, search.institution).map((institution) => (
            <option key={institution.id} value={institution.id}>
              {institution.name}
            </option>
          ))}
        </NativeSelect>
      </label>
      <label className="grid gap-1 text-sm text-muted-foreground">
        {t("accounts.group")}
        <NativeSelect
          onChange={(event) => onPatch({ group: event.target.value || undefined })}
          value={search.group ?? ""}
        >
          <option value="">{t("accounts.allGroups")}</option>
          {filterChoices(groups, search.group).map((group) => (
            <option key={group.id} value={group.id}>
              {group.name}
            </option>
          ))}
        </NativeSelect>
      </label>
    </div>
  );
}

function AccountSummary({ account }: { account: AccountRecordDto }) {
  const { t } = useTranslation();
  const value = account.valuation.base
    ? formatMoney(account.valuation.base.amount, account.valuation.base.currency)
    : t("accounts.noValue");
  const owners = account.owners.map((owner) => owner.memberName).join(", ");
  return (
    <div className="mt-1 space-y-1 text-sm text-muted-foreground">
      <p>{value}</p>
      {account.valuation.complete ? null : <p>{t("quotes.incomplete")}</p>}
      <p>{t(`accounts.categories.${account.secondaryCategory}`)}</p>
      {owners ? <p>{owners}</p> : null}
    </div>
  );
}

function NativeSelect({ className, ...props }: ComponentPropsWithoutRef<"select">) {
  return (
    <select
      {...props}
      className={cn(
        "h-10 w-full rounded-lg border border-border bg-card px-3 text-sm text-foreground shadow-sm outline-none transition-colors focus-visible:ring-2 focus-visible:ring-ring disabled:cursor-not-allowed disabled:opacity-50",
        className,
      )}
    />
  );
}

function filterChoices<T extends { id: string; archivedAt: string | null }>(
  records: T[],
  selectedId: string | undefined,
): T[] {
  return records.filter(
    (record) => record.archivedAt === null || record.id === selectedId,
  );
}

export default AccountsPage;
