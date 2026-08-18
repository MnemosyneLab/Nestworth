import { lazyRouteComponent } from "@tanstack/react-router";

export const SettingsGeneralRoute = lazyRouteComponent(
  () => import("@/features/settings/general-page"),
);
