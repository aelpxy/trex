import { useState, type FormEvent } from "react";
import { redirect, useRevalidator } from "react-router";
import { LuShield } from "react-icons/lu";

import { Button } from "~/components/ui/button";
import { EmptyState } from "~/components/ui/empty-state";
import { Page } from "~/components/ui/page";
import { focusRing } from "~/components/ui/styles";
import { ApiError } from "~/lib/api";
import { dollarsToCredits, formatUsd } from "~/lib/credits";
import { pageTitle } from "~/lib/meta";
import { trex, type ApiAdminWorkspace } from "~/lib/trex";

import type { Route } from "./+types/admin";

export const meta = () => pageTitle("Admin");

// only admins get past the loader; everyone else is sent home
export async function clientLoader() {
  try {
    const [workspaces, plans] = await Promise.all([trex.admin.workspaces(), trex.plans()]);
    return { workspaces, plans };
  } catch (error) {
    if (error instanceof ApiError && error.status === 403) throw redirect("/");
    throw error;
  }
}

const errorText = (cause: unknown) => (cause instanceof Error ? cause.message : String(cause));

function Manage({ workspace, plans, onDone }: { workspace: ApiAdminWorkspace; plans: Route.ComponentProps["loaderData"]["plans"]; onDone: () => Promise<void> }) {
  const [dollars, setDollars] = useState("");
  const [reason, setReason] = useState("");
  const [plan, setPlan] = useState(workspace.plan);
  const [error, setError] = useState<string | null>(null);
  const [result, setResult] = useState<string | null>(null);
  const amount = dollarsToCredits(Number(dollars));

  async function run(action: () => Promise<string>) {
    setError(null);
    setResult(null);
    try {
      setResult(await action());
      await onDone();
    } catch (cause) {
      setError(errorText(cause));
    }
  }

  function adjust(event: FormEvent) {
    event.preventDefault();
    void run(async () => {
      const balance = await trex.admin.adjustCredits(workspace.id, { amount, description: reason.trim() || "Admin adjustment" });
      setDollars("");
      setReason("");
      return `Done. ${workspace.name} now has ${formatUsd(balance.balance)}.`;
    });
  }

  function changePlan(event: FormEvent) {
    event.preventDefault();
    void run(async () => {
      await trex.admin.setPlan(workspace.id, plan);
      return `${workspace.name} is on the ${plans.find((option) => option.id === plan)?.name ?? plan} plan.`;
    });
  }

  return (
    <div className="space-y-5 border-t border-line px-4 py-4">
      <form onSubmit={adjust}>
        <p className="text-xs font-medium">Adjust balance</p>
        <p className="mt-0.5 text-[11px] text-muted">A positive amount adds money, a negative one removes it. It shows in the workspace's activity.</p>
        <div className="mt-2.5 flex gap-2">
          <label className="relative w-36 shrink-0">
            <span className="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-sm text-muted">$</span>
            <input type="number" step="0.01" value={dollars} onChange={(event) => setDollars(event.target.value)} aria-label="Amount in dollars" placeholder="10.00" className={`ui-input h-10 pl-7 ${focusRing}`} />
          </label>
          <input value={reason} onChange={(event) => setReason(event.target.value)} aria-label="Reason" placeholder="Reason" className={`ui-input h-10 ${focusRing}`} />
          <Button type="submit" disabled={!Number.isFinite(amount) || amount === 0}>Apply</Button>
        </div>
      </form>
      {plans.length > 0 && (
        <form onSubmit={changePlan}>
          <p className="text-xs font-medium">Plan</p>
          <div className="mt-2.5 flex gap-2">
            <select value={plan} onChange={(event) => setPlan(event.target.value)} aria-label="Plan" className={`ui-input h-10 ${focusRing}`}>
              {plans.map((option) => (
                <option key={option.id} value={option.id}>
                  {option.name} · {formatUsd(option.monthly_credits)} a month
                </option>
              ))}
            </select>
            <Button type="submit" disabled={plan === workspace.plan}>Set plan</Button>
          </div>
        </form>
      )}
      {error && <p role="alert" className="text-xs text-danger">{error}</p>}
      {result && <p role="status" className="text-xs text-muted">{result}</p>}
    </div>
  );
}

export default function Admin({ loaderData }: Route.ComponentProps) {
  const { workspaces, plans } = loaderData;
  const revalidator = useRevalidator();
  const [query, setQuery] = useState("");
  const [open, setOpen] = useState<string | null>(null);
  const needle = query.trim().toLowerCase();
  const shown = needle
    ? workspaces.filter((workspace) => [workspace.name, workspace.owner_email ?? "", workspace.id].some((text) => text.toLowerCase().includes(needle)))
    : workspaces;
  const planName = (id: string) => plans.find((plan) => plan.id === id)?.name ?? id;

  return (
    <Page title="Admin">
      <p className="mt-2 text-xs text-muted">Every workspace on this server. Open one to change its balance or plan.</p>
      <input
        value={query}
        onChange={(event) => setQuery(event.target.value)}
        aria-label="Filter workspaces"
        placeholder="Filter by name, owner or id"
        className={`ui-input mt-6 h-10 ${focusRing}`}
      />
      {shown.length === 0 ? (
        <EmptyState icon={LuShield} title="No workspaces" description={needle ? "Nothing matches that filter." : "No one has signed up yet."} />
      ) : (
        <ul className="ui-card mt-4 overflow-hidden">
          {shown.map((workspace) => (
            <li key={workspace.id} className="border-b border-line last:border-0">
              <button
                type="button"
                onClick={() => setOpen((current) => (current === workspace.id ? null : workspace.id))}
                aria-expanded={open === workspace.id}
                className={`flex w-full cursor-pointer items-center gap-4 px-4 py-3 text-left text-sm hover:bg-subtle/60 ${focusRing}`}
              >
                <span className="min-w-0 flex-1">
                  <span className="block truncate font-medium">{workspace.name}</span>
                  <span className="block truncate text-xs text-muted">{workspace.owner_email ?? workspace.id}</span>
                </span>
                <span className="shrink-0 text-xs text-muted">{planName(workspace.plan)}</span>
                <span className="w-24 shrink-0 text-right tabular-nums">{formatUsd(workspace.credits)}</span>
              </button>
              {open === workspace.id && <Manage workspace={workspace} plans={plans} onDone={() => revalidator.revalidate()} />}
            </li>
          ))}
        </ul>
      )}
    </Page>
  );
}
