import { useState } from "react";
import { Link, useNavigate } from "react-router";
import { Field } from "@base-ui/react/field";
import { Form } from "@base-ui/react/form";

import { ApiError, setToken } from "~/lib/api";
import { trex } from "~/lib/trex";

import { Button } from "~/components/ui/button";
import { focusRing } from "~/components/ui/styles";

import { PasswordInput } from "./password-input";

export type AuthMode = "signin" | "signup";

const MIN_PASSWORD_LENGTH = 8;

const label = "mb-1.5 block text-xs font-medium text-muted";
const input = `ui-input h-10 ${focusRing}`;
const error = "mt-1.5 text-xs text-danger";

const COPY = {
  signin: { title: "Welcome back", description: "Sign in to continue to your workspace.", submit: "Sign in", switchText: "New to Trex Code?", switchLink: "Create an account" },
  signup: { title: "Create your account", description: "Start building with an agent that has its own sandbox.", submit: "Create account", switchText: "Already have an account?", switchLink: "Sign in" },
};

export function AuthForm({ mode }: { mode: AuthMode }) {
  const navigate = useNavigate();
  const copy = COPY[mode];
  const signup = mode === "signup";
  const [errors, setErrors] = useState<Record<string, string>>({});
  const [busy, setBusy] = useState(false);

  async function submit(values: Record<string, unknown>) {
    const text = (name: string) => String(values[name] ?? "");
    setBusy(true);
    setErrors({});
    try {
      const { token } = signup
        ? await trex.signup({ name: text("name"), email: text("email"), password: text("password") })
        : await trex.login({ email: text("email"), password: text("password") });
      setToken(token);
      navigate("/", { replace: true });
    } catch (cause) {
      // the api names the field it rejected; anything else is shown under the password
      const field = cause instanceof ApiError && cause.param && ["name", "email", "password"].includes(cause.param) ? cause.param : cause instanceof ApiError && cause.status === 409 ? "email" : "password";
      setErrors({ [field]: cause instanceof Error ? cause.message.charAt(0).toUpperCase() + cause.message.slice(1) : "Something went wrong." });
    } finally {
      setBusy(false);
    }
  }

  return (
    <div className="w-full">
      <h1 className="text-xl font-medium tracking-tight">{copy.title}</h1>
      <p className="mt-1 text-sm text-muted">{copy.description}</p>

      <Form key={mode} validationMode="onBlur" errors={errors} onFormSubmit={submit} className="mt-6 space-y-4">
        {signup && (
          <Field.Root name="name">
            <Field.Label className={label}>Name</Field.Label>
            <Field.Control required autoComplete="name" placeholder="Ada Lovelace" className={input} />
            <Field.Error match="valueMissing" className={error}>
              Enter your name.
            </Field.Error>
            <Field.Error className={error} />
          </Field.Root>
        )}

        <Field.Root name="email">
          <Field.Label className={label}>Email</Field.Label>
          <Field.Control type="email" required autoComplete="email" placeholder="you@example.com" className={input} />
          <Field.Error match="valueMissing" className={error}>
            Enter your email.
          </Field.Error>
          <Field.Error match="typeMismatch" className={error}>
            Enter a valid email address.
          </Field.Error>
          <Field.Error className={error} />
        </Field.Root>

        <Field.Root name="password">
          <Field.Label className={label}>Password</Field.Label>
          <PasswordInput required minLength={signup ? MIN_PASSWORD_LENGTH : undefined} autoComplete={signup ? "new-password" : "current-password"} />
          {signup && <Field.Description className="mt-1.5 text-xs text-muted">At least {MIN_PASSWORD_LENGTH} characters.</Field.Description>}
          <Field.Error match="valueMissing" className={error}>
            Enter your password.
          </Field.Error>
          <Field.Error match="tooShort" className={error}>
            Use at least {MIN_PASSWORD_LENGTH} characters.
          </Field.Error>
          <Field.Error className={error} />
        </Field.Root>

        <Button type="submit" disabled={busy} className="mt-2 w-full">
          {copy.submit}
        </Button>
      </Form>

      <p className="mt-6 text-center text-sm text-muted">
        {copy.switchText}{" "}
        <Link to={signup ? "/auth" : "/auth?mode=signup"} className={`rounded-sm font-medium text-ink underline-offset-4 hover:underline ${focusRing}`}>
          {copy.switchLink}
        </Link>
      </p>
    </div>
  );
}
