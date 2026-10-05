import type { ReactNode } from "react";
import { LuMinimize2 } from "react-icons/lu";

import { Button } from "~/components/ui/button";
import { Meter } from "~/components/ui/meter";
import { tokens } from "~/lib/format";

// the agent compacts by itself at this share of the window, and the ring stands out a little before
const AUTO_COMPACT_PERCENT = 80;
const FILLING_PERCENT = 60;
const RING_RADIUS = 19;

export type ContextUsage = {
  used: number | null;
  window: number;
  // none while compacting can't start, e.g. during a run
  onCompact?: () => void;
};

const percentOf = ({ used, window }: ContextUsage) => (used === null ? 0 : Math.min(100, Math.round((used / window) * 100)));

// how full the context is, drawn around the send button so it's seen without taking room
export function ContextRing({ context, children }: { context?: ContextUsage; children: ReactNode }) {
  if (!context) return children;
  const percent = percentOf(context);
  const ring = 2 * Math.PI * RING_RADIUS;
  // nothing goes wrong as it fills, so it only grows more prominent rather than turning red
  const filling = percent >= FILLING_PERCENT;
  return (
    <span className="relative -m-1 inline-flex size-10 shrink-0 items-center justify-center" title={`Context ${percent}% full`}>
      <svg viewBox="0 0 40 40" className="pointer-events-none absolute inset-0 -rotate-90" aria-hidden>
        <circle cx="20" cy="20" r={RING_RADIUS} fill="none" strokeWidth="2" strokeLinecap="round" strokeDasharray={ring} strokeDashoffset={ring * (1 - percent / 100)} className={`transition-[stroke-dashoffset] ${filling ? "stroke-ink" : "stroke-muted/50"}`} />
      </svg>
      {children}
    </span>
  );
}

// offered beside the model once the context fills up, when compacting starts to matter
export function CompactButton({ context }: { context?: ContextUsage }) {
  if (!context?.onCompact || percentOf(context) < FILLING_PERCENT) return null;
  return (
    <Button variant="quiet" size="sm" onClick={context.onCompact} title="Summarize the conversation to free up context" className="shrink-0 tabular-nums">
      <LuMinimize2 size={13} />
      <span className="pointer-coarse:hidden">{percentOf(context)}% ·</span> Compact
    </Button>
  );
}

// the context's details and a way to compact it before it fills up
export function ContextDetails({ context }: { context: ContextUsage }) {
  const { used, window, onCompact } = context;
  const percent = percentOf(context);
  return (
    <div className="px-1">
      <Meter
        label="Context"
        value={used ?? 0}
        max={window}
        detail={used === null ? "Just summarized" : `${tokens(used)} of ${tokens(window)}`}
        valueText={used === null ? "Just summarized" : `${percent}% of the context window`}
      />
      <p className="mt-2 text-xs text-muted">
        Summarized on its own at {AUTO_COMPACT_PERCENT}%.{!onCompact && " You can compact once the current run ends."}
      </p>
      <Button variant="quiet" size="sm" disabled={!onCompact} onClick={onCompact} className="mt-2 w-full">
        <LuMinimize2 size={14} />
        Compact now
      </Button>
    </div>
  );
}
