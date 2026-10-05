import { useState } from "react";
import { Dialog } from "@base-ui/react/dialog";

import { Button } from "~/components/ui/button";
import { SelectField } from "~/components/ui/select-field";
import { backdrop, dialogDescription, dialogPopup, dialogTitle, dialogViewport } from "~/components/ui/styles";

type MoveDialogProps = {
  count: number;
  // the folders it can go to, as `a/b/`; "" is the top of the library
  folders: string[];
  initial: string;
  onMove: (folder: string) => void;
  onClose: () => void;
};

// picks the folder to move files and folders into; mount it to open it
export function MoveDialog({ count, folders, initial, onMove, onClose }: MoveDialogProps) {
  const [folder, setFolder] = useState(initial);
  const options = folders.map((path) => ({ value: path, label: path === "" ? "Library" : `Library / ${path.slice(0, -1).split("/").join(" / ")}` }));

  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className={backdrop} />
        <Dialog.Viewport className={dialogViewport}>
          <Dialog.Popup className={`${dialogPopup} max-w-sm`}>
            <Dialog.Title className={dialogTitle}>Move {count === 1 ? "1 item" : `${count} items`}</Dialog.Title>
            <Dialog.Description className={dialogDescription}>Files keep their names; nothing already in the folder is replaced.</Dialog.Description>
            <div className="mt-5">
              <SelectField label="Folder" options={options} value={folder} onChange={setFolder} />
            </div>
            <div className="mt-6 flex justify-end gap-2">
              <Dialog.Close render={<Button variant="quiet" />}>Cancel</Dialog.Close>
              <Button onClick={() => onMove(folder)} disabled={folder === initial}>
                Move here
              </Button>
            </div>
          </Dialog.Popup>
        </Dialog.Viewport>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
