import { useQuery } from "@tanstack/react-query";

import type { BootstrapDto, CommandError } from "@/generated/tauri-bindings";
import { commands } from "@/generated/tauri-bindings";

export const bootstrapQueryKey = ["bootstrap"] as const;

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
): "/onboarding" | "/overview" | "/startup-error" {
  if (error || bootstrap?.status === "blocked") {
    return "/startup-error";
  }
  if (bootstrap?.status === "ready" && bootstrap.onboardingRequired) {
    return "/onboarding";
  }
  if (bootstrap?.status === "ready") {
    return "/overview";
  }
  return "/onboarding";
}
