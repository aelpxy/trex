import { useState, type FormEvent } from "react";
import { Field } from "@base-ui/react/field";
import { useMutation } from "@tanstack/react-query";
import { useRevalidator } from "react-router";

import { Button } from "~/components/ui/button";
import { fieldLabel, focusRing } from "~/components/ui/styles";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { toastOutcome } from "~/lib/toasts";
import { trex } from "~/lib/trex";


export function ProfileForm() {
  const { profile } = useWorkspace();
  const revalidator = useRevalidator();
  const [name, setName] = useState(profile.name);
  // the profile comes from the app layout's loader, so it reloads to show the new name everywhere
  const update = useMutation({ mutationFn: trex.updateMe, onSuccess: () => revalidator.revalidate() });

  function save(event: FormEvent) {
    event.preventDefault();
    void toastOutcome(update.mutateAsync({ name: name.trim() }), { success: "Name saved", error: "Couldn't save your name" });
  }

  return (
    <form onSubmit={save} className="space-y-3">
      <Field.Root>
        <Field.Label className={fieldLabel}>Name</Field.Label>
        <Field.Control value={name} onValueChange={setName} required autoComplete="name" className={`ui-input h-10 ${focusRing}`} />
      </Field.Root>
      <Field.Root disabled>
        <Field.Label className={fieldLabel}>Email</Field.Label>
        <Field.Control value={profile.email} readOnly className="ui-input h-10" />
      </Field.Root>
      <Button type="submit" disabled={!name.trim() || name.trim() === profile.name || update.isPending}>
        Save
      </Button>
    </form>
  );
}
