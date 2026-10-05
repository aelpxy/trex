import type { ReactNode } from "react";

type StatProps = { label: string; value: ReactNode; note?: ReactNode; size?: "sm" | "lg"; className?: string };

// one figure with its label, inside a <dl>; lg for dashboards, sm for drawers and cards
export function Stat({ label, value, note, size = "sm", className = "" }: StatProps) {
  return (
    <div className={`min-w-0 ${className}`}>
      <dt className="text-xs text-muted">{label}</dt>
      <dd className={`mt-1 truncate font-medium tabular-nums ${size === "lg" ? "text-xl tracking-tight" : "text-sm"}`}>{value}</dd>
      {note && <dd className="mt-0.5 truncate text-xs text-muted">{note}</dd>}
    </div>
  );
}
