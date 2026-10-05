import { useState, type FormEvent } from "react";
import { Field } from "@base-ui/react/field";
import { useSuspenseQuery } from "@tanstack/react-query";

import { Button } from "~/components/ui/button";
import { Checkbox } from "~/components/ui/checkbox";
import { SelectField } from "~/components/ui/select-field";
import { DrawerSection } from "~/components/ui/side-drawer";
import { focusRing } from "~/components/ui/styles";
import { dollarsToCredits, formatUsd } from "~/lib/credits";
import { queries } from "~/lib/queries";
import type { ApiAdminWorkspace } from "~/lib/trex";

import { ActionStatus } from "./action-status";
import { useAdjustFunds, useSetModels, useSetPlan } from "./mutations";

const label = "mb-1.5 block text-[11px] font-medium text-muted";
const input = `ui-input h-10 ${focusRing}`;

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
    <DrawerSection title="Funds" description="Adds to the balance; a negative amount removes funds. It shows in their activity.">
      <form onSubmit={submit} className="space-y-3">
        <div className="flex gap-2">
          <Field.Root className="w-32 shrink-0">
            <Field.Label className={label}>Amount</Field.Label>
            <div className="relative">
              <span className="pointer-events-none absolute top-1/2 left-3.5 -translate-y-1/2 text-sm text-muted">$</span>
              <Field.Control type="number" step="0.01" value={dollars} onValueChange={setDollars} placeholder="10.00" className={`${input} pl-7`} />
            </div>
          </Field.Root>
          <Field.Root className="min-w-0 flex-1">
            <Field.Label className={label}>Reason</Field.Label>
            <Field.Control value={reason} onValueChange={setReason} placeholder="Admin adjustment" className={input} />
          </Field.Root>
        </div>
        <div className="flex items-center justify-end gap-3">
          <ActionStatus error={adjust.error} success={adjust.data ? `New balance ${formatUsd(adjust.data.balance)}.` : null} />
          <Button type="submit" disabled={!Number.isFinite(amount) || amount === 0 || adjust.isPending}>
            Apply
          </Button>
        </div>
      </form>
    </DrawerSection>
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
    <DrawerSection title="Models" description="Only allowed models show in their picker and can run.">
      <form onSubmit={submit}>
        <Checkbox checked={!limited} onCheckedChange={(every) => setLimited(!every)}>
          Every model, including ones added later
        </Checkbox>
        {limited && (
          <div className="mt-2.5 grid gap-2 pl-6 sm:grid-cols-2">
            {models.map((model) => (
              <Checkbox
                key={model.id}
                checked={allowed.includes(model.id)}
                onCheckedChange={(checked) => setAllowed((current) => (checked ? [...current, model.id] : current.filter((id) => id !== model.id)))}
              >
                {model.name}
              </Checkbox>
            ))}
          </div>
        )}
        <div className="mt-3 flex items-center justify-end gap-3">
          <ActionStatus error={save.error} success={save.isSuccess ? "Saved." : null} />
          <Button type="submit" disabled={!changed || (limited && allowed.length === 0) || save.isPending}>
            Save models
          </Button>
        </div>
      </form>
    </DrawerSection>
  );
}

function PlanForm({ workspace }: { workspace: ApiAdminWorkspace }) {
  const { data: plans } = useSuspenseQuery(queries.plans());
  const [plan, setPlan] = useState(workspace.plan);
  const save = useSetPlan();
  if (plans.length === 0) return null;
  const options = plans.map((option) => ({ value: option.id, label: `${option.name} · ${formatUsd(option.monthly_credits)} a month` }));

  return (
    <DrawerSection title="Plan">
      <form
        onSubmit={(event) => {
          event.preventDefault();
          save.mutate({ workspace: workspace.id, plan });
        }}
        className="flex gap-2"
      >
        <div className="min-w-0 flex-1">
          <SelectField label="Plan" options={options} value={plan} onChange={setPlan} />
        </div>
        <Button type="submit" disabled={plan === workspace.plan || save.isPending}>
          Set plan
        </Button>
      </form>
      <div className="mt-2">
        <ActionStatus error={save.error} success={save.isSuccess ? "Saved." : null} />
      </div>
    </DrawerSection>
  );
}

// funds, models and plan of one workspace, as drawer sections
export function WorkspaceManager({ workspace }: { workspace: ApiAdminWorkspace }) {
  return (
    <>
      <FundsForm workspace={workspace} />
      <ModelsForm workspace={workspace} />
      <PlanForm workspace={workspace} />
    </>
  );
}
