import { useState, type DragEvent, type HTMLAttributes } from "react";

import { droppedFiles } from "~/components/library/use-file-drop";

// marks a drag of tree rows, so folders tell it apart from files off the desktop
const TREE_TYPE = "application/x-sandbox-path";

// what is being dragged: a file, or a folder with everything under it
export type Dragged = { path: string; folder: boolean };

export const parentOf = (path: string) => (path.includes("/") ? path.slice(0, path.lastIndexOf("/")) : "");
export const baseName = (path: string) => path.slice(path.lastIndexOf("/") + 1);

// a folder can't go inside itself, and dropping where it already is does nothing
export const canMoveInto = (dragged: Dragged, folder: string) =>
  parentOf(dragged.path) !== folder && !(dragged.folder && (folder === dragged.path || folder.startsWith(`${dragged.path}/`)));

type TreeDragOptions = {
  onMove: (dragged: Dragged, folder: string) => void;
  onUpload: (folder: string, files: File[]) => void;
};

// rows drag onto folders, or onto the tree's empty space for the top level; files from the
// desktop are uploaded where they're dropped
export function useTreeDrag({ onMove, onUpload }: TreeDragOptions) {
  const [dragging, setDragging] = useState<Dragged | null>(null);
  // the folder that would take the drop; "" is the top level
  const [dropTarget, setDropTarget] = useState<string | null>(null);

  const dragProps = (dragged: Dragged): HTMLAttributes<HTMLElement> => ({
    draggable: true,
    onDragStart: (event) => {
      event.stopPropagation();
      setDragging(dragged);
      event.dataTransfer.setData(TREE_TYPE, dragged.path);
      event.dataTransfer.effectAllowed = "move";
    },
    onDragEnd: () => {
      setDragging(null);
      setDropTarget(null);
    },
  });

  const dropProps = (folder: string): HTMLAttributes<HTMLElement> => {
    const accepts = (event: DragEvent) =>
      event.dataTransfer.types.includes(TREE_TYPE) ? dragging !== null && canMoveInto(dragging, folder) : event.dataTransfer.types.includes("Files");
    return {
      onDragOver: (event) => {
        if (!accepts(event)) return;
        // the innermost folder under the pointer takes it, not the ones around it
        event.preventDefault();
        event.stopPropagation();
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
        event.stopPropagation();
        setDropTarget(null);
        if (dragging) onMove(dragging, folder);
        else onUpload(folder, droppedFiles(event.dataTransfer));
        setDragging(null);
      },
    };
  };

  return { dragging, dropTarget, dragProps, dropProps };
}
