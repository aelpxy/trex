import { useState, type FormEvent } from "react";
import { useSuspenseQuery } from "@tanstack/react-query";

import { Button } from "~/components/ui/button";
import { focusRing } from "~/components/ui/styles";
import { dollarsToCredits, formatUsd } from "~/lib/credits";
import { queries } from "~/lib/queries";
import type { ApiAdminWorkspace } from "~/lib/trex";

import { ActionStatus } from "./action-status";
import { useAdjustFunds, useSetModels, useSetPlan } from "./mutations";

function FundsForm({ workspace }: { workspace: ApiAdminWorkspace }) {
  const [dollars, setDollars] = useState("");
  const [reason, setReason] = useState("");
  const adjust = useAdjustFunds();
  const amount = dollarsToCredits(Number(dollars));

  function submit(event: FormEvent) {
    event.preventDefault();
    adjust.mutate(
      { workspace: workspace.id, amount, description: reason.trim() || "Admin adjustment" },
      {
        onSuccess: () => {
          setDollars("");
          setReason("");
        },
      },
    );
  }

  return (
    <form onSubmit={submit}>
      <p className="text-xs font-medium">Add funds</p>
      <p className="mt-0.5 text-[11px] text-muted">Balance {formatUsd(workspace.credits)}. A negative amount removes funds; it shows in their activity.</p>
      <div className="mt-2.5 flex gap-2">
        <label className="relative w-36 shrink-0">
          <span className="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-sm text-muted">$</span>
          <input type="number" step="0.01" value={dollars} onChange={(event) => setDollars(event.target.value)} aria-label="Amount in dollars" placeholder="10.00" className={`ui-input h-10 pl-7 ${focusRing}`} />
        </label>
        <input value={reason} onChange={(event) => setReason(event.target.value)} aria-label="Reason" placeholder="Reason" className={`ui-input h-10 ${focusRing}`} />
        <Button type="submit" disabled={!Number.isFinite(amount) || amount === 0 || adjust.isPending}>
          Apply
        </Button>
      </div>
      <div className="mt-2">
        <ActionStatus error={adjust.error} success={adjust.data ? `New balance ${formatUsd(adjust.data.balance)}.` : null} />
      </div>
    </form>
  );
}

function ModelsForm({ workspace }: { workspace: ApiAdminWorkspace }) {
  const { data: models } = useSuspenseQuery(queries.admin.models());
  const [limited, setLimited] = useState(workspace.allowed_models !== null);
  const [allowed, setAllowed] = useState<string[]>(workspace.allowed_models ?? models.map((model) => model.id));
  const save = useSetModels();
  const sorted = (ids: string[]) => [...ids].sort().join("\n");
  const changed = limited ? workspace.allowed_models === null || sorted(allowed) !== sorted(workspace.allowed_models) : workspace.allowed_models !== null;

  function submit(event: FormEvent) {
    event.preventDefault();
    save.mutate({ workspace: workspace.id, models: limited ? allowed : null });
  }

  return (
    <form onSubmit={submit}>
      <p className="text-xs font-medium">Models</p>
      <p className="mt-0.5 text-[11px] text-muted">Only allowed models show in their picker and can run.</p>
      <label className="mt-2.5 flex items-center gap-2 text-[13px]">
        <input type="checkbox" checked={!limited} onChange={(event) => setLimited(!event.target.checked)} className="accent-(--color-ink)" />
        Every model, including ones added later
      </label>
      {limited && (
        <div className="mt-2 grid gap-1.5 pl-6 sm:grid-cols-2">
          {models.map((model) => (
            <label key={model.id} className="flex items-center gap-2 text-[13px]">
              <input
                type="checkbox"
                checked={allowed.includes(model.id)}
                onChange={(event) => setAllowed((current) => (event.target.checked ? [...current, model.id] : current.filter((id) => id !== model.id)))}
                className="accent-(--color-ink)"
              />
              <span className="truncate">{model.name}</span>
            </label>
          ))}
        </div>
      )}
      <div className="mt-3 flex items-center gap-3">
        <Button type="submit" disabled={!changed || (limited && allowed.length === 0) || save.isPending}>
          Save models
        </Button>
        <ActionStatus error={save.error} success={save.isSuccess ? "Saved." : null} />
      </div>
    </form>
  );
}

function PlanForm({ workspace }: { workspace: ApiAdminWorkspace }) {
  const { data: plans } = useSuspenseQuery(queries.plans());
  const [plan, setPlan] = useState(workspace.plan);
  const save = useSetPlan();
  if (plans.length === 0) return null;

  return (
    <form
      onSubmit={(event) => {
        event.preventDefault();
        save.mutate({ workspace: workspace.id, plan });
      }}
    >
      <p className="text-xs font-medium">Plan</p>
      <div className="mt-2.5 flex gap-2">
        <select value={plan} onChange={(event) => setPlan(event.target.value)} aria-label="Plan" className={`ui-input h-10 ${focusRing}`}>
          {plans.map((option) => (
            <option key={option.id} value={option.id}>
              {option.name} · {formatUsd(option.monthly_credits)} a month
            </option>
          ))}
        </select>
        <Button type="submit" disabled={plan === workspace.plan || save.isPending}>
          Set plan
        </Button>
      </div>
      <div className="mt-2">
        <ActionStatus error={save.error} success={save.isSuccess ? "Saved." : null} />
      </div>
    </form>
  );
}

// funds, models and plan of one workspace
export function WorkspaceManager({ workspace }: { workspace: ApiAdminWorkspace }) {
  return (
    <div className="space-y-6 border-t border-line px-4 py-4">
      <FundsForm workspace={workspace} />
      <ModelsForm workspace={workspace} />
      <PlanForm workspace={workspace} />
    </div>
  );
}
