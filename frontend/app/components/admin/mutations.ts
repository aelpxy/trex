import { useMutation, useQueryClient } from "@tanstack/react-query";

import { ApiError } from "~/lib/api";
import { queries } from "~/lib/queries";
import { trex } from "~/lib/trex";

// every admin change can show up in several views, so the whole admin cache refreshes after one
function useAdminMutation<Variables, Result>(mutationFn: (variables: Variables) => Promise<Result>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn,
    onSettled: () => queryClient.invalidateQueries({ queryKey: queries.admin.all }),
  });
}

export const useAdjustFunds = () =>
  useAdminMutation(({ workspace, amount, description }: { workspace: string; amount: number; description: string }) =>
    trex.admin.adjustCredits(workspace, { amount, description }),
  );

export const useSetPlan = () => useAdminMutation(({ workspace, plan }: { workspace: string; plan: string }) => trex.admin.setPlan(workspace, plan));

export const useSetModels = () =>
  useAdminMutation(({ workspace, models }: { workspace: string; models: string[] | null }) => trex.admin.setModels(workspace, models));

export const useSetRole = () => useAdminMutation(({ user, role }: { user: string; role: "user" | "admin" }) => trex.admin.updateUser(user, { role }));

export const useSetSuspended = () => useAdminMutation(({ user, suspended }: { user: string; suspended: boolean }) => trex.admin.updateUser(user, { suspended }));

export const useSetUserPassword = () => useAdminMutation(({ user, password }: { user: string; password: string }) => trex.admin.setPassword(user, password));

export const useSignOutUser = () => useAdminMutation((user: string) => trex.admin.signOut(user));

export const useEndUserSession = () =>
  useAdminMutation(({ user, session }: { user: string; session: string }) => trex.admin.endSignInSession(user, session));

export const useCancelRun = () => useAdminMutation((session: string) => trex.admin.cancelRun(session));

export const useStopSandbox = () => useAdminMutation(({ workspace, name }: { workspace: string; name: string }) => trex.admin.stopSandbox(workspace, name));

export const useDeleteSandbox = () => useAdminMutation(({ workspace, name }: { workspace: string; name: string }) => trex.admin.deleteSandbox(workspace, name));

export const useDeleteUser = () => useAdminMutation((user: string) => trex.admin.deleteUser(user));

// one at a time, since each tears down sandboxes and files; the list refreshes even when one fails part way
export function useDeleteUsers() {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (users: string[]) => {
      for (const [done, user] of users.entries()) {
        try {
          await trex.admin.deleteUser(user);
        } catch (cause) {
          // another admin got there first
          if (cause instanceof ApiError && cause.status === 404) continue;
          const message = cause instanceof Error ? cause.message : String(cause);
          throw new Error(`Deleted ${done} of ${users.length} users, then: ${message}`);
        }
      }
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: queries.admin.all }),
  });
}
