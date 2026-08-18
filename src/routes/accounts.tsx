import { lazyRouteComponent } from "@tanstack/react-router";

export const AccountsRoute = lazyRouteComponent(
  () => import("@/features/accounts/accounts-page"),
);
