import { StartupErrorPage } from "@/features/startup/startup-error-page";
import { useBootstrapQuery } from "@/lib/tauri/bootstrap";

export function StartupErrorRoute() {
  const bootstrap = useBootstrapQuery();
  return <StartupErrorPage bootstrap={bootstrap.data} error={bootstrap.error} />;
}

export default StartupErrorRoute;
