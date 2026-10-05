import type { HTMLAttributes } from "react";
import { LuChevronRight } from "react-icons/lu";

import { focusRing } from "~/components/ui/styles";

type BreadcrumbsProps = {
  folder: string;
  onOpen: (folder: string) => void;
  // makes each step a place to drop files onto, highlighted while `dropTarget` is its folder
  dropProps?: (folder: string) => HTMLAttributes<HTMLElement>;
  dropTarget?: string | null;
};

// the path to the open folder, each step a button back to it
export function Breadcrumbs({ folder, onOpen, dropProps, dropTarget }: BreadcrumbsProps) {
  const parts = folder.split("/").filter(Boolean);
  const crumb = (path: string, current: boolean) =>
    `rounded px-1 hover:text-ink ${focusRing} ${current ? "font-medium text-ink" : "cursor-pointer"} ${dropTarget === path ? "bg-accent/10 text-ink outline-2 -outline-offset-2 outline-accent" : ""}`;
  return (
    <nav aria-label="Folder" className="flex min-w-0 flex-wrap items-center gap-0.5 text-xs text-muted">
      <button type="button" {...dropProps?.("")} onClick={() => onOpen("")} className={crumb("", parts.length === 0)}>
        Library
      </button>
      {parts.map((part, index) => {
        const last = index === parts.length - 1;
        const path = `${parts.slice(0, index + 1).join("/")}/`;
        return (
          <span key={index} className="flex items-center gap-0.5">
            <LuChevronRight size={12} className="shrink-0" />
            <button type="button" {...dropProps?.(path)} onClick={() => onOpen(path)} aria-current={last ? "location" : undefined} className={crumb(path, last)}>
              {part}
            </button>
          </span>
        );
      })}
    </nav>
  );
}
