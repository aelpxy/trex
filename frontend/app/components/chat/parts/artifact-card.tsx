import { Button } from "@base-ui/react/button";
import { LuAppWindow, LuChevronRight, LuDownload, LuFileText } from "react-icons/lu";

import { useFiles } from "~/components/files/files-provider";
import { focusRing, iconButton } from "~/components/ui/styles";
import { saveLibraryFile } from "~/lib/library-links";
import { previewOf } from "~/lib/preview";

const KIND = { react: "Interactive app", html: "Web page", markdown: "Document" };

// a previewable file the reply links to, opened in the files panel like an artifact
export function ArtifactCard({ title, path }: { title: string; path: string }) {
  const { openLibrary } = useFiles();
  const preview = previewOf(path);
  if (!preview) return null;
  const Icon = preview === "markdown" ? LuFileText : LuAppWindow;
  const open = () => openLibrary(path).catch((error) => console.warn("could not open the library file", error));

  return (
    <div className="flex items-center overflow-hidden rounded-xl border border-line bg-surface/60">
      <Button onClick={open} className={`group flex min-w-0 flex-1 cursor-pointer items-center gap-3 px-3 py-2.5 text-left transition-colors hover:bg-subtle/60 ${focusRing}`}>
        <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-subtle text-muted">
          <Icon size={17} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block truncate text-sm font-medium">{title}</span>
          <span className="block truncate text-xs text-muted">
            {KIND[preview]} · <span className="font-mono text-[11px]">{path}</span>
          </span>
        </span>
        <span className="flex shrink-0 items-center gap-1 text-xs text-muted group-hover:text-ink">
          Open
          <LuChevronRight size={13} />
        </span>
      </Button>
      <Button
        onClick={() => saveLibraryFile(path).catch((error) => console.warn("could not download the library file", error))}
        aria-label={`Download ${path}`}
        title="Download"
        className={`${iconButton} mx-1.5`}
      >
        <LuDownload size={15} />
      </Button>
    </div>
  );
}
