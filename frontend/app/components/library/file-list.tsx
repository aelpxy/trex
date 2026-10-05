import { Button } from "@base-ui/react/button";
import { LuDownload, LuFile, LuTrash2 } from "react-icons/lu";

import { iconButton } from "~/components/ui/styles";
import type { ApiFile } from "~/lib/trex";

const UNITS = ["B", "KB", "MB", "GB"];

function formatSize(bytes: number) {
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < UNITS.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${unit === 0 ? size : size.toFixed(1)} ${UNITS[unit]}`;
}

const formatDate = (seconds: number) => new Date(seconds * 1000).toLocaleDateString(undefined, { month: "short", day: "numeric", year: "numeric" });

type FileListProps = { files: ApiFile[]; onDownload: (file: ApiFile) => void; onDelete?: (file: ApiFile) => void };

export function FileList({ files, onDownload, onDelete }: FileListProps) {
  return (
    <ul aria-label="Files" className="ui-card mt-8 divide-y divide-line overflow-hidden">
      {files.map((file) => (
        <li key={file.path} className="flex items-center gap-3 px-4 py-2.5">
          <LuFile size={15} className="shrink-0 text-muted" />
          <span className="min-w-0 flex-1 truncate font-mono text-[13px]" title={file.path}>
            {file.path}
          </span>
          <span className="hidden shrink-0 text-xs text-muted tabular-nums sm:block">{formatSize(file.size)}</span>
          <span className="hidden w-24 shrink-0 text-right text-xs text-muted tabular-nums sm:block">{formatDate(file.modified_at)}</span>
          <span className="flex shrink-0 items-center">
            <Button onClick={() => onDownload(file)} aria-label={`Download ${file.path}`} title="Download" className={iconButton}>
              <LuDownload size={14} />
            </Button>
            {onDelete && (
              <Button onClick={() => onDelete(file)} aria-label={`Delete ${file.path}`} title="Delete" className={`${iconButton} hover:text-danger`}>
                <LuTrash2 size={14} />
              </Button>
            )}
          </span>
        </li>
      ))}
    </ul>
  );
}
