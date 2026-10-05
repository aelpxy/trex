import { AlertDialog } from "@base-ui/react/alert-dialog";

import { Button } from "~/components/ui/button";
import { backdrop, dialogDescription, dialogPopup, dialogTitle, dialogViewport } from "~/components/ui/styles";

import { plural } from "./entries";

type ReplaceDialogProps = { names: string[]; onReplace: () => void; onKeepBoth: () => void; onCancel: () => void };

// uploads that would overwrite files ask first; mount it to open it
export function ReplaceDialog({ names, onReplace, onKeepBoth, onCancel }: ReplaceDialogProps) {
  const single = names.length === 1;
  return (
    <AlertDialog.Root open onOpenChange={(open) => !open && onCancel()}>
      <AlertDialog.Portal>
        <AlertDialog.Backdrop className={backdrop} />
        <AlertDialog.Viewport className={dialogViewport}>
          <AlertDialog.Popup className={`${dialogPopup} max-w-sm`}>
            <AlertDialog.Title className={dialogTitle}>{single ? "Replace the file?" : `Replace ${plural(names.length, "file")}?`}</AlertDialog.Title>
            <AlertDialog.Description className={dialogDescription}>
              {single ? `“${names[0]}” is already here.` : `${names.slice(0, 3).join(", ")}${names.length > 3 ? ` and ${names.length - 3} more` : ""} are already here.`} Replacing overwrites{" "}
              {single ? "it" : "them"}; keeping both adds a number to the new {single ? "name" : "names"}.
            </AlertDialog.Description>
            <div className="mt-6 flex flex-wrap justify-end gap-2">
              <AlertDialog.Close render={<Button variant="quiet" />}>Cancel</AlertDialog.Close>
              <Button variant="quiet" onClick={onKeepBoth}>
                Keep both
              </Button>
              <Button onClick={onReplace}>Replace</Button>
            </div>
          </AlertDialog.Popup>
        </AlertDialog.Viewport>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
