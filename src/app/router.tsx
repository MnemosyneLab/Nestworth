import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router";

import { RootRoute } from "@/routes/__root";
import { IndexRoute } from "@/routes/index";

const rootRoute = createRootRoute({ component: RootRoute });
const indexRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/",
  component: IndexRoute,
});

const routeTree = rootRoute.addChildren([indexRoute]);

export const router = createRouter({
  routeTree,
  defaultPreload: "intent",
});

declare module "@tanstack/react-router" {
  interface Register {
    router: typeof router;
  }
}
