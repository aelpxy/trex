import { useState, type FormEvent } from "react";
import { Field } from "@base-ui/react/field";
import { useMutation } from "@tanstack/react-query";

import { PasswordInput } from "~/components/auth/password-input";
import { Button } from "~/components/ui/button";
import { fieldLabel } from "~/components/ui/styles";
import { toasts } from "~/lib/toasts";
import { trex } from "~/lib/trex";

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
          toasts.add({ title: "Password changed", description: "Your other devices were signed out.", type: "success" });
        },
      },
    );
  }

  // a wrong current password belongs to that field, so the error shows there rather than in a toast
  return (
    <form onSubmit={save} className="space-y-3">
      <Field.Root invalid={change.isError}>
        <Field.Label className={fieldLabel}>Current password</Field.Label>
        <PasswordInput autoComplete="current-password" value={current} onValueChange={setCurrent} required />
        {change.error && (
          <p role="alert" className="mt-1.5 text-xs text-danger">
            {change.error.message.charAt(0).toUpperCase() + change.error.message.slice(1)}
          </p>
        )}
      </Field.Root>
      <Field.Root>
        <Field.Label className={fieldLabel}>New password</Field.Label>
        <PasswordInput autoComplete="new-password" minLength={MIN_PASSWORD_LENGTH} value={next} onValueChange={setNext} required />
      </Field.Root>
      <Button type="submit" disabled={!current || next.length < MIN_PASSWORD_LENGTH || change.isPending}>
        Change password
      </Button>
    </form>
  );
}
