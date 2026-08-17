import type { AccountRecordDto } from "@/generated/tauri-bindings";

export const SHARED_OWNER = "shared";

export type AccountSearch = {
  owner?: string;
  category?: string;
  institution?: string;
  group?: string;
};

export function validateAccountSearch(
  search: Record<string, unknown>,
): AccountSearch {
  const next: AccountSearch = {};
  const owner = readString(search.owner);
  const category = readString(search.category);
  const institution = readString(search.institution);
  const group = readString(search.group);
  if (owner) {
    next.owner = owner;
  }
  if (category) {
    next.category = category;
  }
  if (institution) {
    next.institution = institution;
  }
  if (group) {
    next.group = group;
  }
  return next;
}

export function mergeAccountSearch(
  prev: AccountSearch | Record<string, unknown>,
  patch: Partial<AccountSearch>,
): AccountSearch {
  return validateAccountSearch({ ...prev, ...patch });
}

export function accountMatchesSearch(
  account: AccountRecordDto,
  search: AccountSearch,
): boolean {
  if (search.owner === SHARED_OWNER) {
    if (account.owners.length < 2) {
      return false;
    }
  } else if (search.owner) {
    if (
      account.owners.length !== 1 ||
      account.owners[0]?.memberId !== search.owner
    ) {
      return false;
    }
  }
  if (search.category && account.primaryCategory !== search.category) {
    return false;
  }
  if (search.institution && account.institutionId !== search.institution) {
    return false;
  }
  if (search.group && account.groupId !== search.group) {
    return false;
  }
  return true;
}

function readString(value: unknown): string | undefined {
  return typeof value === "string" && value.length > 0 ? value : undefined;
}
