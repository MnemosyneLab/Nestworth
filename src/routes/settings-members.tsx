import { lazyRouteComponent } from "@tanstack/react-router";

export const MembersRoute = lazyRouteComponent(
  () => import("@/features/members/members-page"),
);
