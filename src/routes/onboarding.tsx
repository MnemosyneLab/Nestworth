import { lazyRouteComponent } from "@tanstack/react-router";

export const OnboardingRoute = lazyRouteComponent(
  () => import("@/features/onboarding/onboarding-page"),
);
