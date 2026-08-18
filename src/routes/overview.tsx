import { OverviewPage } from "@/features/overview/overview-page";
import { referenceCatalogFromBootstrap } from "@/lib/reference-catalog";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";

export function OverviewRoute() {
  const bootstrap = useBootstrapQuery();
  if (bootstrap.data?.status !== "ready" || !bootstrap.data.household) {
    return null;
  }

  return (
    <OverviewPage
      catalog={referenceCatalogFromBootstrap(bootstrap.data)}
      household={bootstrap.data.household}
    />
  );
}

export default OverviewRoute;
