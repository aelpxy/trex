import { useState, type FormEvent } from "react";
import { Dialog } from "@base-ui/react/dialog";
import { Field } from "@base-ui/react/field";

import { Button } from "~/components/ui/button";
import { backdrop, dialogDescription, dialogPopup, dialogTitle, dialogViewport, focusRing } from "~/components/ui/styles";

type NameDialogProps = {
  title: string;
  description: string;
  initial?: string;
  action: string;
  onSubmit: (name: string) => void;
  onClose: () => void;
};

// asks for one file or folder name, for new folders and renames; mount it to open it
export function NameDialog({ title, description, initial = "", action, onSubmit, onClose }: NameDialogProps) {
  const [name, setName] = useState(initial);
  const trimmed = name.trim();
  const invalid = trimmed.includes("/") ? "Names can't contain a slash." : trimmed === "." || trimmed === ".." ? "Pick another name." : null;

  function submit(event: FormEvent) {
    event.preventDefault();
    if (!trimmed || invalid) return;
    onSubmit(trimmed);
  }

  return (
    <Dialog.Root open onOpenChange={(open) => !open && onClose()}>
      <Dialog.Portal>
        <Dialog.Backdrop className={backdrop} />
        <Dialog.Viewport className={dialogViewport}>
          <Dialog.Popup className={`${dialogPopup} max-w-sm`}>
            <Dialog.Title className={dialogTitle}>{title}</Dialog.Title>
            <Dialog.Description className={dialogDescription}>{description}</Dialog.Description>
            <form onSubmit={submit} className="mt-5">
              <Field.Root invalid={invalid !== null}>
                <Field.Label className="mb-1.5 block text-xs font-medium text-muted">Name</Field.Label>
                <Field.Control value={name} onValueChange={setName} required autoFocus autoComplete="off" spellCheck={false} className={`ui-input h-10 ${focusRing}`} />
                {invalid && <p className="mt-1.5 text-xs text-danger">{invalid}</p>}
              </Field.Root>
              <div className="mt-6 flex justify-end gap-2">
                <Dialog.Close render={<Button variant="quiet" />}>Cancel</Dialog.Close>
                <Button type="submit" disabled={!trimmed || invalid !== null || trimmed === initial}>
                  {action}
                </Button>
              </div>
            </form>
          </Dialog.Popup>
        </Dialog.Viewport>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
