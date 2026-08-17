import { createRootRoute, createRoute, createRouter } from "@tanstack/react-router";

import { validateAccountSearch } from "@/features/accounts/search";
import { RootRoute } from "@/routes/__root";
import { AccountsRoute } from "@/routes/accounts";
import { AccountDetailRoute } from "@/routes/accounts-detail";
import { GroupsRoute } from "@/routes/groups";
import { IndexRoute } from "@/routes/index";
import { InstitutionsRoute } from "@/routes/institutions";
import { OnboardingRoute } from "@/routes/onboarding";
import { OverviewRoute } from "@/routes/overview";
import { SettingsGeneralRoute } from "@/routes/settings-general";
import { MembersRoute } from "@/routes/settings-members";
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
const accountsRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/accounts",
  component: AccountsRoute,
  validateSearch: validateAccountSearch,
});
const accountDetailRoute = createRoute({
  getParentRoute: () => rootRoute,
  path: "/accounts/$accountId",
  component: AccountDetailRoute,
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
  component: StartupErrorRoute,
});

const routeTree = rootRoute.addChildren([
  indexRoute,
  onboardingRoute,
  overviewRoute,
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
