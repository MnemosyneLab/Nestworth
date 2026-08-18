import type { QueryClient } from "@tanstack/react-query";

import { bootstrapQueryKey } from "@/lib/tauri/bootstrap";

export async function invalidateValuation(
  queryClient: QueryClient,
  accountId?: string,
) {
  await Promise.all([
    queryClient.invalidateQueries({ queryKey: ["overview"] }),
    queryClient.invalidateQueries({ queryKey: ["accounts"] }),
    queryClient.invalidateQueries({ queryKey: ["portfolio"] }),
    queryClient.invalidateQueries({ queryKey: ["required-fx"] }),
    queryClient.invalidateQueries({ queryKey: ["instruments"] }),
    queryClient.invalidateQueries({ queryKey: bootstrapQueryKey }),
    accountId
      ? queryClient.invalidateQueries({ queryKey: ["account", accountId] })
      : Promise.resolve(),
    accountId
      ? queryClient.invalidateQueries({ queryKey: ["holdings", accountId] })
      : Promise.resolve(),
    accountId
      ? queryClient.invalidateQueries({ queryKey: ["cash", accountId] })
      : Promise.resolve(),
  ]);
}
