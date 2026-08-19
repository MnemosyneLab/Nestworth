import { useQuery } from "@tanstack/react-query";

import type { BootstrapDto, CommandError } from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";

export const bootstrapQueryKey = ["bootstrap"] as const;

export const APP_ROUTES = [
  "/overview",
  "/investments",
  "/activity",
  "/analytics",
  "/instruments",
  "/accounts",
  "/institutions",
  "/groups",
  "/settings/general",
  "/settings/members",
] as const;

export type AppRoute = (typeof APP_ROUTES)[number];
export type GateRoute = AppRoute | "/onboarding" | "/startup-error";

function isOnboardedPath(pathname: string): boolean {
  if ((APP_ROUTES as readonly string[]).includes(pathname)) {
    return true;
  }
  return /^\/accounts\/[^/]+$/.test(pathname);
}

export async function loadBootstrap(): Promise<BootstrapDto> {
  const result = await commands.bootstrap();
  if (result.status === "error") {
    throw result.error;
  }
  return result.data;
}

export function useBootstrapQuery() {
  return useQuery({
    queryKey: bootstrapQueryKey,
    queryFn: loadBootstrap,
    retry: false,
    staleTime: Infinity,
  });
}

export function destinationForBootstrap(
  bootstrap: BootstrapDto | undefined,
  error: CommandError | null,
  pathname: string,
): GateRoute {
  if (error || bootstrap?.status === "blocked") {
    return "/startup-error";
  }
  if (bootstrap?.status === "ready" && bootstrap.onboardingRequired) {
    return "/onboarding";
  }
  if (bootstrap?.status === "ready") {
    if (isOnboardedPath(pathname)) {
      return pathname as AppRoute;
    }
    return "/overview";
  }
  return "/onboarding";
}
