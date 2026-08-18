import { lazyRouteComponent } from "@tanstack/react-router";

export const GroupsRoute = lazyRouteComponent(
  () => import("@/features/groups/groups-page"),
);
