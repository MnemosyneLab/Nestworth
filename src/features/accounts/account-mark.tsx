import {
  Banknote,
  CreditCard,
  Home,
  TrendingUp,
  Wallet,
  type LucideIcon,
} from "lucide-react";

import { MediaImage } from "@/features/media/media-image";
import type {
  AccountRecordDto,
  InstitutionRecordDto,
} from "@/generated/tauri-bindings";

const CATEGORY_ICONS: Record<string, LucideIcon> = {
  cash_equivalent: Wallet,
  investment: TrendingUp,
  property: Home,
  receivable: Banknote,
  liability: CreditCard,
};

export function resolvedAccountLogoId(
  account: AccountRecordDto,
  institutions: InstitutionRecordDto[],
): string | null {
  if (account.logoAssetId) {
    return account.logoAssetId;
  }
  return (
    institutions.find((institution) => institution.id === account.institutionId)
      ?.logoAssetId ?? null
  );
}

export function AccountMark({
  account,
  institutions,
}: {
  account: AccountRecordDto;
  institutions: InstitutionRecordDto[];
}) {
  const assetId = resolvedAccountLogoId(account, institutions);
  if (assetId) {
    return <MediaImage alt="" assetId={assetId} />;
  }
  const Icon = CATEGORY_ICONS[account.primaryCategory] ?? Wallet;
  return (
    <span
      aria-hidden="true"
      className="inline-flex h-10 w-10 items-center justify-center rounded-lg bg-surface-soft text-muted-foreground"
    >
      <Icon size={16} />
    </span>
  );
}
