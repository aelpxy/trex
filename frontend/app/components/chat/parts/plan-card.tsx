import { LuCircle, LuCircleCheck, LuCircleDot, LuListChecks } from "react-icons/lu";

import type { PlanPart } from "../types";

const STATUS_LABEL = { pending: "To do", in_progress: "In progress", completed: "Done" };

// the agent's checklist, updated in place as it works
export function PlanCard({ part }: { part: PlanPart }) {
  const done = part.steps.filter((step) => step.status === "completed").length;
  return (
    <section aria-label="Plan" className="rounded-xl border border-line bg-surface/60 px-3 py-2.5">
      <header className="flex items-center gap-2 text-xs">
        <LuListChecks size={15} className="shrink-0 text-muted" />
        <span className="font-medium">Plan</span>
        <span className="ml-auto text-[11px] text-muted tabular-nums">
          {done} of {part.steps.length} done
        </span>
      </header>
      {part.explanation && <p className="mt-1.5 text-xs text-muted">{part.explanation}</p>}
      <ol className="mt-2 space-y-1.5">
        {part.steps.map((step, index) => (
          <li key={`${index}-${step.step}`} className="flex items-start gap-2 text-[13px] leading-5">
            <span className="mt-0.5 shrink-0">
              {step.status === "completed" ? (
                <LuCircleCheck size={14} className="text-muted" />
              ) : step.status === "in_progress" ? (
                <LuCircleDot size={14} className="text-ink" />
              ) : (
                <LuCircle size={14} className="text-muted/60" />
              )}
            </span>
            <span className={step.status === "completed" ? "text-muted line-through decoration-muted/50" : step.status === "in_progress" ? "font-medium" : ""}>{step.step}</span>
            <span className="sr-only">{STATUS_LABEL[step.status]}</span>
          </li>
        ))}
      </ol>
    </section>
  );
}
