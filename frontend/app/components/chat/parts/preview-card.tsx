import { Button } from "@base-ui/react/button";
import { LuAppWindow, LuChevronRight } from "react-icons/lu";

import { useFiles } from "~/components/files/files-provider";
import { focusRing } from "~/components/ui/styles";

import type { PreviewPart } from "../types";

export function PreviewCard({ part }: { part: PreviewPart }) {
  const { openPreview, previewError, canBrowse } = useFiles();
  return (
    <div>
      <Button
        onClick={() => void openPreview(part.port, part.path)}
        disabled={!canBrowse}
        className={`group flex w-full cursor-pointer items-center gap-3 rounded-xl border border-line bg-surface/60 px-3 py-2.5 text-left transition-colors hover:bg-subtle/60 ${focusRing}`}
      >
        <span className="flex size-9 shrink-0 items-center justify-center rounded-lg bg-subtle text-muted">
          <LuAppWindow size={17} />
        </span>
        <span className="min-w-0 flex-1">
          <span className="block text-sm font-medium">Live preview</span>
          <span className="block truncate font-mono text-[11px] text-muted">
            localhost:{part.port}
            {part.path}
          </span>
        </span>
        <span className="flex shrink-0 items-center gap-1 text-xs text-muted group-hover:text-ink">
          Open
          <LuChevronRight size={13} />
        </span>
      </Button>
      {previewError && (
        <p role="alert" className="mt-1.5 text-xs text-danger">
          {previewError}
        </p>
      )}
    </div>
  );
}
