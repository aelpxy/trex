import { useState, type FormEvent } from "react";
import { Dialog } from "@base-ui/react/dialog";
import { Field } from "@base-ui/react/field";
import { LuPlus } from "react-icons/lu";

import { Button } from "~/components/ui/button";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { backdrop, dialogDescription, dialogPopup, dialogTitle, dialogViewport, focusRing } from "~/components/ui/styles";

export function NewProjectDialog() {
  const [open, setOpen] = useState(false);
  const { createProject: create } = useWorkspace();

  function createProject(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    const name = String(new FormData(event.currentTarget).get("name") ?? "").trim();
    if (!name) return;
    setOpen(false);
    create(name).catch((error) => console.warn("could not create the project", error));
  }

  return (
    <Dialog.Root open={open} onOpenChange={setOpen}>
      <Dialog.Trigger
        aria-label="New project"
        title="New project"
        className={`inline-flex size-6 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted opacity-0 transition hover:bg-subtle hover:text-ink group-hover/section:opacity-100 focus-visible:opacity-100 data-popup-open:opacity-100 pointer-coarse:opacity-100 ${focusRing}`}
      >
        <LuPlus size={14} />
      </Dialog.Trigger>
      <Dialog.Portal>
        <Dialog.Backdrop className={backdrop} />
        <Dialog.Viewport className={dialogViewport}>
          <Dialog.Popup className={`${dialogPopup} max-w-sm`}>
            <Dialog.Title className={dialogTitle}>New project</Dialog.Title>
            <Dialog.Description className={dialogDescription}>Projects group related chats together.</Dialog.Description>
            <form onSubmit={createProject} className="mt-5">
              <Field.Root name="name">
                <Field.Label className="mb-1.5 block text-xs font-medium text-muted">Name</Field.Label>
                <Field.Control required autoFocus autoComplete="off" placeholder="My project" className={`ui-input h-10 ${focusRing}`} />
              </Field.Root>
              <div className="mt-6 flex justify-end gap-2">
                <Dialog.Close render={<Button variant="quiet" />}>Cancel</Dialog.Close>
                <Button type="submit">Create</Button>
              </div>
            </form>
          </Dialog.Popup>
        </Dialog.Viewport>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
