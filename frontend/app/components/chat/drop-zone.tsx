import { useRef, useState, type DragEvent, type ReactNode } from "react";
import { LuUpload } from "react-icons/lu";

const hasFiles = (event: DragEvent) => event.dataTransfer.types.includes("Files");

// files dropped anywhere on the chat are attached to the message being written
export function DropZone({ onFiles, children }: { onFiles: (files: File[]) => void; children: ReactNode }) {
  const [active, setActive] = useState(false);
  // enter and leave fire for every child the drag crosses, so only the outermost pair counts
  const depth = useRef(0);

  return (
    <div
      className="relative flex min-w-0 flex-1 flex-col"
      onDragEnter={(event) => {
        if (!hasFiles(event)) return;
        event.preventDefault();
        depth.current += 1;
        setActive(true);
      }}
      onDragOver={(event) => {
        if (!hasFiles(event)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = "copy";
      }}
      onDragLeave={(event) => {
        if (!hasFiles(event)) return;
        depth.current = Math.max(0, depth.current - 1);
        if (depth.current === 0) setActive(false);
      }}
      onDrop={(event) => {
        if (!hasFiles(event)) return;
        event.preventDefault();
        depth.current = 0;
        setActive(false);
        onFiles([...event.dataTransfer.files]);
      }}
    >
      {children}
      {active && (
        <div aria-hidden className="pointer-events-none absolute inset-3 z-30 flex flex-col items-center justify-center gap-2 rounded-2xl border-2 border-dashed border-muted/50 bg-surface/80 backdrop-blur-sm">
          <LuUpload size={22} className="text-muted" />
          <p className="text-sm font-medium">Drop files to attach</p>
          <p className="text-xs text-muted">Images, PDFs and text files</p>
        </div>
      )}
    </div>
  );
}
