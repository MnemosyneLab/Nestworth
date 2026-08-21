import { lazyRouteComponent } from "@tanstack/react-router";

export const MaintenanceRoute = lazyRouteComponent(
  () => import("@/features/maintenance/maintenance-page"),
);
