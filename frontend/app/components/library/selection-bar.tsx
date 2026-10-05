import { LuFolderInput, LuTrash2, LuX } from "react-icons/lu";

import { Button } from "~/components/ui/button";

import { plural } from "./entries";

type SelectionBarProps = { count: number; onClear: () => void; onMove: () => void; onDelete: () => void };

// what to do with the ticked rows; it takes the breadcrumbs' place while anything is ticked
export function SelectionBar({ count, onClear, onMove, onDelete }: SelectionBarProps) {
  return (
    <div role="toolbar" aria-label="Selected items" className="flex w-full items-center gap-2">
      <Button variant="quiet" onClick={onClear} aria-label="Clear selection" title="Clear selection" className="h-8 px-2">
        <LuX size={14} />
      </Button>
      <p className="text-xs font-medium tabular-nums">{plural(count, "item")} selected</p>
      <span className="flex-1" />
      <Button variant="quiet" onClick={onMove} className="h-8 px-3 text-xs">
        <LuFolderInput size={14} />
        Move
      </Button>
      <Button variant="subtleDanger" onClick={onDelete} className="h-8 px-3 text-xs">
        <LuTrash2 size={14} />
        Delete
      </Button>
    </div>
  );
}
