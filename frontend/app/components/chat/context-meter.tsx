import { Meter } from "@base-ui/react/meter";
import { Popover } from "@base-ui/react/popover";
import { LuMinimize2 } from "react-icons/lu";

import { Button } from "~/components/ui/button";
import { focusRingOutset, popup } from "~/components/ui/styles";

// the agent compacts by itself at this share of the window, so the meter warns a little before
const AUTO_COMPACT_PERCENT = 80;
const WARN_PERCENT = 60;

const tokens = (value: number) => (value >= 1_000_000 ? `${(value / 1_000_000).toFixed(1)}M` : value >= 1000 ? `${Math.round(value / 1000)}k` : String(value));

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
  const warn = percent >= WARN_PERCENT;

  return (
    <Popover.Root>
      <Popover.Trigger
        aria-label={used === null ? "Context usage" : `Context ${percent}% full`}
        className={`inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md px-2 text-xs text-muted tabular-nums transition-colors hover:bg-subtle hover:text-ink data-popup-open:bg-subtle ${focusRingOutset}`}
      >
        <svg viewBox="0 0 18 18" className="size-4 -rotate-90" aria-hidden>
          <circle cx="9" cy="9" r="7" fill="none" strokeWidth="2.5" className="stroke-line" />
          <circle cx="9" cy="9" r="7" fill="none" strokeWidth="2.5" strokeLinecap="round" strokeDasharray={ring} strokeDashoffset={ring * (1 - percent / 100)} className={warn ? "stroke-danger" : "stroke-muted"} />
        </svg>
        <span className="pointer-coarse:hidden">{used === null ? "—" : `${percent}%`}</span>
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner side="top" align="end" sideOffset={8} className="z-50">
          <Popover.Popup className={`w-72 rounded-xl p-4 ${popup}`}>
            <Popover.Title className="text-sm font-medium">Context</Popover.Title>
            <Meter.Root value={used ?? 0} max={window} className="mt-3">
              <div className="mb-1.5 flex items-baseline justify-between text-xs">
                <Meter.Label className="text-muted">Used</Meter.Label>
                <span className="text-muted tabular-nums">{used === null ? "Just summarized" : `${tokens(used)} of ${tokens(window)}`}</span>
              </div>
              <Meter.Track className="h-1.5 overflow-hidden rounded-full bg-subtle">
                <Meter.Indicator className={`h-full rounded-full transition-[width] ${warn ? "bg-danger" : "bg-ink"}`} />
              </Meter.Track>
            </Meter.Root>
            <Popover.Description className="mt-3 text-xs text-muted">
              At {AUTO_COMPACT_PERCENT}% the conversation is summarized on its own so the agent can keep going. Compact earlier to start the next message from a short summary.
            </Popover.Description>
            <Popover.Close
              render={
                <Button variant="quiet" disabled={!onCompact} onClick={onCompact} className="mt-3 h-8 w-full px-3 text-xs">
                  <LuMinimize2 size={13} />
                  {onCompact ? "Compact now" : "Available once the run ends"}
                </Button>
              }
            />
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
