import { AlertDialog } from "@base-ui/react/alert-dialog";

import { Button } from "./button";
import { backdrop, dialogDescription, dialogPopup, dialogTitle, dialogViewport } from "./styles";

export type DeleteTarget = { kind: "project" | "chat" | "file" | "task" | "user" | "users" | "folder" | "items" | "sandbox" | "sandboxes"; id: string; name: string };

type DeleteConfirmDialogProps = {
  target: DeleteTarget | null;
  consequence?: string;
  onConfirm: (target: DeleteTarget) => void;
  onCancel: () => void;
};

export function DeleteConfirmDialog({ target, consequence = "", onConfirm, onCancel }: DeleteConfirmDialogProps) {
  return (
    <AlertDialog.Root open={target !== null} onOpenChange={(open) => !open && onCancel()}>
      <AlertDialog.Portal>
        <AlertDialog.Backdrop className={backdrop} />
        <AlertDialog.Viewport className={dialogViewport}>
          <AlertDialog.Popup className={`${dialogPopup} max-w-sm`}>
            <AlertDialog.Title className={dialogTitle}>Delete {target?.kind}?</AlertDialog.Title>
            <AlertDialog.Description className={dialogDescription}>
              “{target?.name}”{consequence} will be permanently deleted.
            </AlertDialog.Description>
            <div className="mt-6 flex justify-end gap-2">
              <AlertDialog.Close render={<Button variant="quiet" />}>Cancel</AlertDialog.Close>
              <Button variant="danger" onClick={() => target && onConfirm(target)}>
                Delete
              </Button>
            </div>
          </AlertDialog.Popup>
        </AlertDialog.Viewport>
      </AlertDialog.Portal>
    </AlertDialog.Root>
  );
}
