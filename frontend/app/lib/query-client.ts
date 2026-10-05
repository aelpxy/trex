import { QueryClient } from "@tanstack/react-query";

import { ApiError } from "./api";

const STALE_MS = 30_000;
const RETRIES = 2;

// one client for the whole app, so route loaders can prefetch into the same cache components read
export const queryClient = new QueryClient({
  defaultOptions: {
    queries: {
      staleTime: STALE_MS,
      // a 4xx won't change by asking again
      retry: (failures, error) => failures < RETRIES && !(error instanceof ApiError && error.status < 500),
    },
  },
});
