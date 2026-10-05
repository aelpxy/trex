import { Button } from "@base-ui/react/button";
import { LuFolderTree } from "react-icons/lu";

import { focusRing } from "~/components/ui/styles";

import { useFiles } from "./files-provider";

export function FilesButton() {
  const { paths, panelOpen, showPanel, close } = useFiles();
  if (paths.length === 0) return null;

  return (
    <Button
      onClick={panelOpen ? close : showPanel}
      aria-expanded={panelOpen}
      className={`inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-xs font-medium text-muted transition-colors hover:bg-subtle hover:text-ink aria-expanded:bg-subtle aria-expanded:text-ink ${focusRing}`}
    >
      <LuFolderTree size={14} />
      Files
      <span className="rounded bg-subtle px-1 font-mono text-[10px] tabular-nums">{paths.length}</span>
    </Button>
  );
}
