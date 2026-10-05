import { Popover } from "@base-ui/react/popover";
import { LuMinimize2 } from "react-icons/lu";

import { Button } from "~/components/ui/button";
import { Meter } from "~/components/ui/meter";
import { focusRingOutset, popup } from "~/components/ui/styles";
import { tokens } from "~/lib/format";

// the agent compacts by itself at this share of the window, and the ring stands out a little before
const AUTO_COMPACT_PERCENT = 80;
const FILLING_PERCENT = 60;

type ContextMeterProps = {
  used: number | null;
  window: number;
  // none while compacting can't start, e.g. during a run
  onCompact?: () => void;
};

// how full the conversation's context is, with a way to compact it before it fills up
export function ContextMeter({ used, window, onCompact }: ContextMeterProps) {
  const percent = used === null ? 0 : Math.min(100, Math.round((used / window) * 100));
  const ring = 2 * Math.PI * 7;
  // nothing goes wrong as it fills, so it only grows more prominent rather than turning red
  const filling = percent >= FILLING_PERCENT;

  return (
    <Popover.Root>
      <Popover.Trigger
        aria-label={used === null ? "Context usage" : `Context ${percent}% full`}
        className={`inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md px-2 text-xs text-muted tabular-nums transition-colors hover:bg-subtle hover:text-ink data-popup-open:bg-subtle ${focusRingOutset}`}
      >
        <svg viewBox="0 0 18 18" className="size-4 -rotate-90" aria-hidden>
          <circle cx="9" cy="9" r="7" fill="none" strokeWidth="2.5" className="stroke-line" />
          <circle cx="9" cy="9" r="7" fill="none" strokeWidth="2.5" strokeLinecap="round" strokeDasharray={ring} strokeDashoffset={ring * (1 - percent / 100)} className={filling ? "stroke-ink" : "stroke-muted"} />
        </svg>
        <span>{used === null ? "—" : `${percent}%`}</span>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner side="top" align="end" sideOffset={8} className="z-50">
          <Popover.Popup className={`w-72 rounded-lg p-4 ${popup}`}>
            <Popover.Title className="mb-3 text-sm font-medium">Context</Popover.Title>
            <Meter
              label="Used"
              value={used ?? 0}
              max={window}
              detail={used === null ? "Just summarized" : `${tokens(used)} of ${tokens(window)}`}
              valueText={used === null ? "Just summarized" : `${percent}% of the context window`}
            />
            <Popover.Description className="mt-3 text-xs text-muted">
              At {AUTO_COMPACT_PERCENT}% the conversation is summarized on its own so the agent can keep going. Compact earlier to start the next message from a short summary.
              {!onCompact && " You can compact once the current run ends."}
            </Popover.Description>
            <Popover.Close
              render={
                <Button variant="quiet" size="sm" disabled={!onCompact} onClick={onCompact} className="mt-3 w-full">
                  <LuMinimize2 size={14} />
                  Compact now
                </Button>
              }
            />
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
