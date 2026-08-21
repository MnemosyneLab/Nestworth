import {
  createRootRoute,
  createRoute,
  createRouter,
  lazyRouteComponent,
} from "@tanstack/react-router";

import { validateAccountSearch } from "@/features/accounts/search";
import { validateActivitySearch } from "@/features/activity/search";
import { validateAnalyticsSearch } from "@/features/analytics/search";
import { RootRoute } from "@/routes/__root";
import { AccountsRoute } from "@/routes/accounts";
import { GroupsRoute } from "@/routes/groups";
import { IndexRoute } from "@/routes/index";
import { InstitutionsRoute } from "@/routes/institutions";
import { InstrumentsRoute } from "@/routes/instruments";
import { InvestmentsRoute } from "@/routes/investments";
import { OnboardingRoute } from "@/routes/onboarding";
import { SettingsGeneralRoute } from "@/routes/settings-general";
import { MembersRoute } from "@/routes/settings-members";
import { MaintenanceRoute } from "@/routes/maintenance";

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
  component: lazyRouteComponent(() => import("@/routes/overview")),
});
const investmentsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/investments",
  component: InvestmentsRoute,
});
const activityRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/activity",
  component: lazyRouteComponent(() => import("@/routes/activity")),
  validateSearch: validateActivitySearch,
});
const analyticsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/analytics",
  component: lazyRouteComponent(() => import("@/routes/analytics")),
  validateSearch: validateAnalyticsSearch,
});
const maintenanceRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/maintenance",
  component: MaintenanceRoute,
});
const instrumentsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/instruments",
  component: InstrumentsRoute,
});
const accountsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/accounts",
  component: AccountsRoute,
  validateSearch: validateAccountSearch,
});
const accountDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/accounts/$accountId",
  component: lazyRouteComponent(() => import("@/routes/accounts-detail")),
  validateSearch: validateAccountSearch,
});
const institutionsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/institutions",
  component: InstitutionsRoute,
});
const groupsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/groups",
  component: GroupsRoute,
});
const settingsGeneralRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings/general",
  component: SettingsGeneralRoute,
});
const membersRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/settings/members",
  component: MembersRoute,
});
const startupErrorRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/startup-error",
  component: lazyRouteComponent(() => import("@/routes/startup-error")),
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  onboardingRoute,
  overviewRoute,
  investmentsRoute,
  activityRoute,
  analyticsRoute,
  maintenanceRoute,
  instrumentsRoute,
  accountsRoute,
  accountDetailRoute,
  institutionsRoute,
  groupsRoute,
  settingsGeneralRoute,
  membersRoute,
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
