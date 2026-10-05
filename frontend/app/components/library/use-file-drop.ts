import { useRef, useState, type DragEvent } from "react";

// files from the desktop dropped anywhere on the manager; `over` shows the drop hint
export function useFileDrop(onFiles: (files: File[]) => void) {
  const [over, setOver] = useState(false);
  // dragenter and dragleave fire for every child crossed, so count them
  const depth = useRef(0);
  const accepts = (event: DragEvent) => event.dataTransfer.types.includes("Files");
  return {
    over,
    props: {
      onDragEnter: (event: DragEvent) => {
        if (!accepts(event)) return;
        event.preventDefault();
        depth.current += 1;
        setOver(true);
      },
      onDragOver: (event: DragEvent) => {
        if (!accepts(event)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = "copy";
      },
      onDragLeave: (event: DragEvent) => {
        if (!accepts(event)) return;
        depth.current = Math.max(0, depth.current - 1);
        if (depth.current === 0) setOver(false);
      },
      onDrop: (event: DragEvent) => {
        if (!accepts(event)) return;
        depth.current = 0;
        setOver(false);
        // a folder or breadcrumb took it already
        if (event.defaultPrevented) return;
        event.preventDefault();
        onFiles([...event.dataTransfer.files]);
      },
    },
  };
}
