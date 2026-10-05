import { useState, type FormEvent } from "react";
import { Dialog } from "@base-ui/react/dialog";
import { Field } from "@base-ui/react/field";

import { Button } from "./button";
import { backdrop, dialogDescription, dialogPopup, dialogTitle, dialogViewport, fieldLabel, focusRing } from "./styles";

type NameDialogProps = {
  title: string;
  description: string;
  initial?: string;
  action: string;
  label?: string;
  // why a name can't be used, or null when it can
  validate?: (name: string) => string | null;
  onSubmit: (name: string) => void;
  onClose: () => void;
};

// asks for one name, for new folders and renames; mount it to open it
export function NameDialog({ title, description, initial = "", action, label = "Name", validate, onSubmit, onClose }: NameDialogProps) {
  const [name, setName] = useState(initial);
  const trimmed = name.trim();
  const invalid = trimmed ? (validate?.(trimmed) ?? null) : null;

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
                <Field.Label className={fieldLabel}>{label}</Field.Label>
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
