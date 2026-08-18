import { lazyRouteComponent } from "@tanstack/react-router";

export const InstitutionsRoute = lazyRouteComponent(
  () => import("@/features/institutions/institutions-page"),
);
