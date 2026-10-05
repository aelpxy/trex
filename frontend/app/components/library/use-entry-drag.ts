import { useState, type DragEvent, type HTMLAttributes } from "react";

import { canDrop, keyOf, type Entry } from "./entries";

// marks a drag of library rows, so drop targets tell them apart from files off the desktop
const ENTRIES_TYPE = "application/x-library-entries";

type EntryDragOptions = {
  // the rows a drag of `entry` carries: it, or the selection it's part of
  carried: (entry: Entry) => Entry[];
  onMove: (entries: Entry[], folder: string) => void;
  onUpload: (folder: string, files: File[]) => void;
};

// rows drag onto folders (rows or breadcrumbs) to move; files from the desktop drop onto them too
export function useEntryDrag({ carried, onMove, onUpload }: EntryDragOptions) {
  const [dragging, setDragging] = useState<Entry[] | null>(null);
  // the folder under the pointer that would take the drop
  const [dropTarget, setDropTarget] = useState<string | null>(null);

  const dragProps = (entry: Entry): HTMLAttributes<HTMLElement> => ({
    draggable: true,
    onDragStart: (event) => {
      const entries = carried(entry);
      setDragging(entries);
      event.dataTransfer.setData(ENTRIES_TYPE, entries.map(keyOf).join("\n"));
      event.dataTransfer.effectAllowed = "move";
    },
    onDragEnd: () => {
      setDragging(null);
      setDropTarget(null);
    },
  });

  const dropProps = (folder: string): HTMLAttributes<HTMLElement> => {
    const accepts = (event: DragEvent) =>
      event.dataTransfer.types.includes(ENTRIES_TYPE) ? dragging !== null && canDrop(folder, dragging) : event.dataTransfer.types.includes("Files");
    return {
      onDragOver: (event) => {
        if (!accepts(event)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = dragging ? "move" : "copy";
        setDropTarget(folder);
      },
      onDragLeave: (event) => {
        if (event.currentTarget.contains(event.relatedTarget as Node | null)) return;
        setDropTarget((current) => (current === folder ? null : current));
      },
      onDrop: (event) => {
        if (!accepts(event)) return;
        event.preventDefault();
        setDropTarget(null);
        if (dragging) onMove(dragging, folder);
        else onUpload(folder, [...event.dataTransfer.files]);
        setDragging(null);
      },
    };
  };

  const isDragged = (entry: Entry) => dragging?.some((dragged) => keyOf(dragged) === keyOf(entry)) ?? false;

  return { dropTarget, dragProps, dropProps, isDragged };
}
