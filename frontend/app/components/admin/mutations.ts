import { useMutation, useQueryClient } from "@tanstack/react-query";

import { queries } from "~/lib/queries";
import { trex } from "~/lib/trex";

// every admin change can show up in several views, so the whole admin cache refreshes after one
function useAdminMutation<Variables, Result>(mutationFn: (variables: Variables) => Promise<Result>) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn,
    onSuccess: () => queryClient.invalidateQueries({ queryKey: queries.admin.all }),
  });
}

export const useAdjustFunds = () =>
  useAdminMutation(({ workspace, amount, description }: { workspace: string; amount: number; description: string }) =>
    trex.admin.adjustCredits(workspace, { amount, description }),
  );

export const useSetPlan = () => useAdminMutation(({ workspace, plan }: { workspace: string; plan: string }) => trex.admin.setPlan(workspace, plan));

export const useSetModels = () =>
  useAdminMutation(({ workspace, models }: { workspace: string; models: string[] | null }) => trex.admin.setModels(workspace, models));

export const useSetRole = () => useAdminMutation(({ user, role }: { user: string; role: "user" | "admin" }) => trex.admin.setRole(user, role));

export const useSignOutUser = () => useAdminMutation((user: string) => trex.admin.signOut(user));

export const useEndUserSession = () =>
  useAdminMutation(({ user, session }: { user: string; session: string }) => trex.admin.endSignInSession(user, session));
