import type { ReactNode } from "react";
import { Meter as BaseMeter } from "@base-ui/react/meter";

type MeterProps = { label: ReactNode; value: number; max: number; detail?: ReactNode; valueText: string };

// a labelled bar for how much of something is used or left
export function Meter({ label, value, max, detail, valueText }: MeterProps) {
  return (
    <BaseMeter.Root value={Math.max(0, Math.min(value, max))} min={0} max={Math.max(max, 1)} getAriaValueText={() => valueText}>
      <div className="mb-1.5 flex items-baseline justify-between gap-3 text-xs">
        <BaseMeter.Label className="min-w-0 truncate text-muted">{label}</BaseMeter.Label>
        {detail && <span className="shrink-0 text-muted tabular-nums">{detail}</span>}
      </div>
      <BaseMeter.Track className="h-1.5 overflow-hidden rounded-full bg-subtle">
        <BaseMeter.Indicator className="h-full rounded-full bg-ink transition-[width]" />
      </BaseMeter.Track>
    </BaseMeter.Root>
  );
}
