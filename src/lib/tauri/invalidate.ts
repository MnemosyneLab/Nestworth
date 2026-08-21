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
    queryClient.invalidateQueries({ queryKey: ["activities"] }),
    queryClient.invalidateQueries({ queryKey: ["activity"] }),
    queryClient.invalidateQueries({ queryKey: ["account-timeline"] }),
    queryClient.invalidateQueries({ queryKey: ["history-origin"] }),
    queryClient.invalidateQueries({ queryKey: ["history-status"] }),
    queryClient.invalidateQueries({ queryKey: ["net-worth-trend"] }),
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

export async function invalidateMarketData(
  queryClient: QueryClient,
  instrumentId?: string,
) {
  await Promise.all([
    invalidateValuation(queryClient),
    queryClient.invalidateQueries({ queryKey: ["instrument-quotes"] }),
    queryClient.invalidateQueries({ queryKey: ["fx-quotes"] }),
    queryClient.invalidateQueries({ queryKey: ["market-data-coverage"] }),
    instrumentId
      ? queryClient.invalidateQueries({ queryKey: ["instrument-quotes", instrumentId] })
      : Promise.resolve(),
  ]);
}
