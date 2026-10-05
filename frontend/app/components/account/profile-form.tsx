import { useState, type FormEvent } from "react";
import { useMutation } from "@tanstack/react-query";
import { useRevalidator } from "react-router";

import { Button } from "~/components/ui/button";
import { focusRing } from "~/components/ui/styles";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { trex } from "~/lib/trex";

import { Status } from "./section";

export function ProfileForm() {
  const { profile } = useWorkspace();
  const revalidator = useRevalidator();
  const [name, setName] = useState(profile.name);
  // the profile comes from the app layout's loader, so it reloads to show the new name everywhere
  const update = useMutation({ mutationFn: trex.updateMe, onSuccess: () => revalidator.revalidate() });

  function save(event: FormEvent) {
    event.preventDefault();
    update.mutate({ name: name.trim() });
  }

  return (
    <form onSubmit={save} className="space-y-3">
      <label className="block">
        <span className="mb-1.5 block text-xs font-medium text-muted">Name</span>
        <input value={name} onChange={(event) => setName(event.target.value)} required className={`ui-input ${focusRing}`} />
      </label>
      <label className="block">
        <span className="mb-1.5 block text-xs font-medium text-muted">Email</span>
        <input value={profile.email} disabled className="ui-input" />
      </label>
      <div className="flex items-center gap-3">
        <Button type="submit" disabled={!name.trim() || name.trim() === profile.name || update.isPending}>
          Save
        </Button>
        <Status error={update.error} saved={update.isSuccess ? "Saved" : null} />
      </div>
    </form>
  );
}
