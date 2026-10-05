import { infiniteQueryOptions, queryOptions } from "@tanstack/react-query";

import { trex } from "./trex";

const LOG_POLL_MS = 2000;

// every server read the app caches, keyed so a change can invalidate exactly what it affects
export const queries = {
  credits: () => queryOptions({ queryKey: ["credits"], queryFn: trex.credits }),
  ledger: () =>
    infiniteQueryOptions({
      queryKey: ["credits", "ledger"],
      queryFn: ({ pageParam }) => trex.ledger(pageParam),
      initialPageParam: undefined as string | undefined,
      getNextPageParam: (page) => (page.has_more ? page.data.at(-1)?.id : undefined),
    }),
  plans: () => queryOptions({ queryKey: ["plans"], queryFn: trex.plans }),
  signInSessions: () => queryOptions({ queryKey: ["me", "sessions"], queryFn: trex.signInSessions }),
  admin: {
    all: ["admin"] as const,
    overview: () => queryOptions({ queryKey: ["admin", "overview"], queryFn: trex.admin.overview }),
    users: () => queryOptions({ queryKey: ["admin", "users"], queryFn: trex.admin.users }),
    signInSessions: (user: string) => queryOptions({ queryKey: ["admin", "users", user, "sessions"], queryFn: () => trex.admin.signInSessions(user) }),
    workspaces: () => queryOptions({ queryKey: ["admin", "workspaces"], queryFn: trex.admin.workspaces }),
    models: () => queryOptions({ queryKey: ["admin", "models"], queryFn: trex.admin.models }),
    library: (workspace: string) => queryOptions({ queryKey: ["admin", "library", workspace], queryFn: () => trex.admin.library(workspace) }),
    usage: (days: number) => queryOptions({ queryKey: ["admin", "usage", days], queryFn: () => trex.admin.usage(days) }),
    logs: () => queryOptions({ queryKey: ["admin", "logs"], queryFn: trex.admin.logs, refetchInterval: LOG_POLL_MS, staleTime: 0 }),
  },
};
