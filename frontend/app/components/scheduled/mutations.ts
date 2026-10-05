import { useMutation, useQueryClient } from "@tanstack/react-query";

import { queries } from "~/lib/queries";
import { trex } from "~/lib/trex";

// any change to a task shows in the list, its page and its runs, so they all refresh
function useTaskMutation<Variables, Result>(mutationFn: (variables: Variables) => Promise<Result>) {
  const queryClient = useQueryClient();
  return useMutation({ mutationFn, onSuccess: () => queryClient.invalidateQueries({ queryKey: queries.scheduled.all }) });
}

export const useRunTask = () => useTaskMutation((id: string) => trex.scheduled.run(id));

export const usePauseTask = () => useTaskMutation(({ id, paused }: { id: string; paused: boolean }) => trex.scheduled.update(id, { paused }));

export const useDeleteTask = () => useTaskMutation((id: string) => trex.scheduled.remove(id));
