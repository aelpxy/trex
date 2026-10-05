import { useState, type FormEvent } from "react";
import { useMutation } from "@tanstack/react-query";

import { Button } from "~/components/ui/button";
import { focusRing } from "~/components/ui/styles";
import { trex } from "~/lib/trex";

import { Status } from "./section";

export const MIN_PASSWORD_LENGTH = 8;

export function PasswordForm() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const change = useMutation({ mutationFn: trex.changePassword });

  function save(event: FormEvent) {
    event.preventDefault();
    change.mutate(
      { current_password: current, new_password: next },
      {
        onSuccess: () => {
          setCurrent("");
          setNext("");
        },
      },
    );
  }

  return (
    <form onSubmit={save} className="space-y-3">
      <label className="block">
        <span className="mb-1.5 block text-xs font-medium text-muted">Current password</span>
        <input type="password" autoComplete="current-password" value={current} onChange={(event) => setCurrent(event.target.value)} required className={`ui-input ${focusRing}`} />
      </label>
      <label className="block">
        <span className="mb-1.5 block text-xs font-medium text-muted">New password</span>
        <input
          type="password"
          autoComplete="new-password"
          minLength={MIN_PASSWORD_LENGTH}
          value={next}
          onChange={(event) => setNext(event.target.value)}
          required
          className={`ui-input ${focusRing}`}
        />
      </label>
      <div className="flex items-center gap-3">
        <Button type="submit" disabled={!current || next.length < MIN_PASSWORD_LENGTH || change.isPending}>
          Change password
        </Button>
        <Status error={change.error} saved={change.isSuccess ? "Password changed. Other devices were signed out." : null} />
      </div>
    </form>
  );
}
