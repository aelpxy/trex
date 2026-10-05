import { redirect, useSearchParams } from "react-router";

import { AppearanceProvider } from "~/components/appearance/appearance-provider";
import { Background } from "~/components/appearance/background";
import { AuthForm, type AuthMode } from "~/components/auth/auth-form";
import { Brand } from "~/components/ui/brand";
import { pageTitle } from "~/lib/meta";
import { signedIn } from "~/lib/trex";

import type { Route } from "./+types/auth";

export async function clientLoader() {
  if (await signedIn()) throw redirect("/");
  return null;
}

export const meta = ({ location }: Route.MetaArgs) => pageTitle(new URLSearchParams(location.search).get("mode") === "signup" ? "Create account" : "Sign in");

export default function Auth() {
  const [params] = useSearchParams();
  const mode: AuthMode = params.get("mode") === "signup" ? "signup" : "signin";

  return (
    <AppearanceProvider>
      <Background />
      <main className="flex min-h-svh flex-col items-center justify-center px-4 py-10">
        <div className="mb-8">
          <Brand />
        </div>
        <div className="glass w-full max-w-sm rounded-panel p-8 shadow-lg ring-1 ring-line">
          <AuthForm mode={mode} />
        </div>
        <p className="mt-6 max-w-sm text-center text-xs text-muted">By continuing you agree to the Terms of Service and Privacy Policy.</p>
      </main>
    </AppearanceProvider>
  );
}
