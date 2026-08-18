import { lazyRouteComponent } from "@tanstack/react-router";

export const InvestmentsRoute = lazyRouteComponent(
  () => import("@/features/investments/investments-page"),
);
