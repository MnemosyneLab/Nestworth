import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router";

import { RootRoute } from "@/routes/__root";
import { IndexRoute } from "@/routes/index";
import { OnboardingRoute } from "@/routes/onboarding";
import { OverviewRoute } from "@/routes/overview";
import { StartupErrorRoute } from "@/routes/startup-error";

const rootRoute = createRootRoute({ component: RootRoute });
const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: IndexRoute,
});
const onboardingRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/onboarding",
  component: OnboardingRoute,
});
const overviewRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/overview",
  component: OverviewRoute,
});
const startupErrorRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/startup-error",
  component: StartupErrorRoute,
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  onboardingRoute,
  overviewRoute,
  startupErrorRoute,
]);

export function createAppRouter() {
  return createRouter({
    routeTree,
    defaultPreload: "intent",
  });
}

export const router = createAppRouter();

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
