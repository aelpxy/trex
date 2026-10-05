import { queryOptions } from "@tanstack/react-query";

import { trex } from "./trex";

const LOG_POLL_MS = 2000;
export const LEDGER_PAGE_SIZE = 10;
export const ADMIN_PAGE_SIZE = 20;

// every server read the app caches, keyed so a change can invalidate exactly what it affects
export const queries = {
  credits: () => queryOptions({ queryKey: ["credits"], queryFn: trex.credits }),
  monthUsage: () => queryOptions({ queryKey: ["credits", "usage"], queryFn: trex.monthUsage }),
  ledger: (page: number) => queryOptions({ queryKey: ["credits", "ledger", page], queryFn: () => trex.ledger(page, LEDGER_PAGE_SIZE) }),
  scheduled: {
    all: ["scheduled"] as const,
    list: () => queryOptions({ queryKey: ["scheduled", "list"], queryFn: trex.scheduled.list }),
    task: (id: string) => queryOptions({ queryKey: ["scheduled", "task", id], queryFn: () => trex.scheduled.get(id) }),
    runs: (id: string) => queryOptions({ queryKey: ["scheduled", "runs", id], queryFn: () => trex.scheduled.runs(id) }),
  },
  library: () => queryOptions({ queryKey: ["library"], queryFn: trex.files }),
  plans: () => queryOptions({ queryKey: ["plans"], queryFn: trex.plans }),
  signInSessions: () => queryOptions({ queryKey: ["me", "sessions"], queryFn: trex.signInSessions }),
  admin: {
    all: ["admin"] as const,
    overview: () => queryOptions({ queryKey: ["admin", "overview"], queryFn: trex.admin.overview }),
    users: (page: number, search: string, sort = "") =>
      queryOptions({ queryKey: ["admin", "users", { page, search, sort }], queryFn: () => trex.admin.users(page, ADMIN_PAGE_SIZE, search, sort) }),
    signInSessions: (user: string) => queryOptions({ queryKey: ["admin", "users", user, "sessions"], queryFn: () => trex.admin.signInSessions(user) }),
    workspaces: (page: number, search: string, sort = "") =>
      queryOptions({ queryKey: ["admin", "workspaces", { page, search, sort }], queryFn: () => trex.admin.workspaces(page, ADMIN_PAGE_SIZE, search, sort) }),
    models: () => queryOptions({ queryKey: ["admin", "models"], queryFn: trex.admin.models }),
    library: (workspace: string) => queryOptions({ queryKey: ["admin", "library", workspace], queryFn: () => trex.admin.library(workspace) }),
    usage: (days: number) => queryOptions({ queryKey: ["admin", "usage", days], queryFn: () => trex.admin.usage(days) }),
    logs: () => queryOptions({ queryKey: ["admin", "logs"], queryFn: trex.admin.logs, refetchInterval: LOG_POLL_MS, staleTime: 0 }),
  },
};
