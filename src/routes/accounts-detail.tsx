import { useParams } from "@tanstack/react-router";

import { AccountDetailPage } from "@/features/accounts/account-detail-page";

export function AccountDetailRoute() {
  const { accountId } = useParams({ from: "/accounts/$accountId" });
  return <AccountDetailPage accountId={accountId} />;
}

export default AccountDetailRoute;
