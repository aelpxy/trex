import type { ReactNode } from "react";
import { LuX } from "react-icons/lu";

import { iconButton } from "./styles";

type SelectionBarProps = { summary: string; onClear: () => void; children: ReactNode };

// what to do with the ticked rows of a table, shown while anything is ticked
export function SelectionBar({ summary, onClear, children }: SelectionBarProps) {
  return (
    <div role="toolbar" aria-label="Selection" className="flex h-10 w-full items-center gap-2 rounded-xl bg-subtle/60 px-1">
      <button type="button" onClick={onClear} aria-label="Clear selection" title="Clear selection" className={iconButton}>
        <LuX size={14} />
      </button>
      <p className="text-xs font-medium tabular-nums">{summary}</p>
      <span className="flex-1" />
      {children}
    </div>
  );
}
