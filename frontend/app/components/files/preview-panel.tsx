import { useEffect, useRef, useState, type FormEvent } from "react";
import { Button } from "@base-ui/react/button";
import { LuAppWindow, LuExternalLink, LuRotateCw, LuX } from "react-icons/lu";

import { focusRing, iconButton } from "~/components/ui/styles";

import { useFiles } from "./files-provider";

// a server running in the sandbox, shown live; it lives on its own origin, so it can't reach trex
export function PreviewPanel() {
  const { preview, closePreview } = useFiles();
  const [path, setPath] = useState(preview?.path ?? "/");
  const [shown, setShown] = useState(preview?.path ?? "/");
  const [reloads, setReloads] = useState(0);
  const closeButton = useRef<HTMLButtonElement>(null);

  useEffect(() => {
    if (!preview) return;
    setPath(preview.path);
    setShown(preview.path);
    closeButton.current?.focus();
  }, [preview]);

  if (!preview) return null;
  const src = new URL(shown.replace(/^\/*/, "/"), preview.url).toString();

  function navigate(event: FormEvent) {
    event.preventDefault();
    const next = path.trim() ? (path.trim().startsWith("/") ? path.trim() : `/${path.trim()}`) : "/";
    setPath(next);
    setShown(next);
    setReloads((count) => count + 1);
  }

  return (
    <aside
      aria-label="Preview"
      onKeyDown={(event) => {
        if (event.key === "Escape" && !event.defaultPrevented && event.currentTarget.contains(event.target as Node)) closePreview();
      }}
      className="glass fixed inset-0 z-40 flex min-h-0 flex-col overflow-hidden md:relative md:z-auto md:w-[52%] md:max-w-4xl md:min-w-[32rem] md:border-l md:border-line"
    >
      <header className="flex h-12 shrink-0 items-center gap-2 border-b border-line px-2">
        <LuAppWindow size={15} className="ml-1.5 shrink-0 text-muted" />
        <form onSubmit={navigate} className="flex min-w-0 flex-1 items-center rounded-md bg-subtle/60 px-2.5">
          <span className="shrink-0 font-mono text-xs text-muted select-none">localhost:{preview.port}</span>
          <input
            value={path}
            onChange={(event) => setPath(event.target.value)}
            aria-label="Path"
            spellCheck={false}
            className={`h-8 min-w-0 flex-1 bg-transparent font-mono text-xs text-ink outline-none ${focusRing}`}
          />
        </form>
        <div className="flex shrink-0 items-center">
          <Button onClick={() => setReloads((count) => count + 1)} aria-label="Reload preview" title="Reload" className={iconButton}>
            <LuRotateCw size={14} />
          </Button>
          <a href={src} target="_blank" rel="noopener noreferrer" aria-label="Open in a new tab" title="Open in a new tab" className={iconButton}>
            <LuExternalLink size={14} />
          </a>
          <Button ref={closeButton} onClick={closePreview} aria-label="Close preview" title="Close" className={iconButton}>
            <LuX size={15} />
          </Button>
        </div>
      </header>
      <iframe
        key={reloads}
        title={`Preview of port ${preview.port}`}
        src={src}
        sandbox="allow-scripts allow-same-origin allow-forms allow-modals allow-popups allow-downloads"
        className="min-h-0 w-full flex-1 bg-white"
      />
    </aside>
  );
}
