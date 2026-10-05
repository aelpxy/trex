import { useMutation, useQueryClient } from "@tanstack/react-query";

import { upload } from "~/lib/api";
import { queries } from "~/lib/queries";
import { trex } from "~/lib/trex";

import type { Move } from "./entries";

// runs each step in turn so a failure says how far it got; the list refreshes either way
function useSteps<T>(step: (item: T) => Promise<unknown>, verb: string) {
  const queryClient = useQueryClient();
  return useMutation({
    mutationFn: async (items: T[]) => {
      for (const [done, item] of items.entries()) {
        try {
          await step(item);
        } catch (cause) {
          const message = cause instanceof Error ? cause.message : String(cause);
          throw new Error(items.length === 1 ? message : `${verb} ${done} of ${items.length} files, then: ${message}`);
        }
      }
    },
    onSettled: () => queryClient.invalidateQueries({ queryKey: queries.library().queryKey }),
  });
}

export const useUploadFiles = () => useSteps(({ path, file }: { path: string; file: File }) => upload(path, file), "Uploaded");

export const useMoveFiles = () => useSteps(({ from, to }: Move) => trex.moveFile(from, to), "Moved");

export const useDeleteFiles = () => useSteps((path: string) => trex.deleteFile(path), "Deleted");
