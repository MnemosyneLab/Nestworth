import { OverviewPage } from "@/features/overview/overview-page";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";

export function OverviewRoute() {
  const bootstrap = useBootstrapQuery();
  if (bootstrap.data?.status !== "ready" || !bootstrap.data.household) {
    return null;
  }

  return (
    <OverviewPage
      household={bootstrap.data.household}
      members={bootstrap.data.members}
    />
  );
}
