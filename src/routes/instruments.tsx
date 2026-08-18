import { lazyRouteComponent } from "@tanstack/react-router";

export const InstrumentsRoute = lazyRouteComponent(
  () => import("@/features/instruments/instruments-page"),
);
