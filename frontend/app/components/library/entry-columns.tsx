import { columnsFor } from "~/components/ui/data-table";
import { date, plural } from "~/lib/format";

import { entryIcon, formatSize, type Entry } from "./entries";
import { EntryRowMenu, type EntryActions } from "./entry-menu";

const column = columnsFor<Entry>();

// name, size and modified, shared by the file manager and the admin library
export const fileColumns = [
  column.accessor("name", {
    header: "Name",
    cell: ({ row }) => {
      const Icon = entryIcon(row.original);
      return (
        <span className="flex min-w-0 items-center gap-3">
          <Icon size={16} className="shrink-0 text-muted" />
          <span className="min-w-0">
            <span className={`block truncate ${row.original.kind === "folder" ? "font-medium" : ""}`} title={row.original.path}>
              {row.original.name}
            </span>
            {row.original.kind === "folder" && <span className="block text-xs text-muted">{plural(row.original.files, "file")}</span>}
          </span>
        </span>
      );
    },
  }),
  column.accessor("size", { header: "Size", cell: (info) => <span className="text-xs text-muted">{formatSize(info.getValue())}</span>, meta: { align: "right", className: "hidden sm:table-cell" } }),
  column.accessor("modified", { header: "Modified", cell: (info) => <span className="text-xs text-muted">{date(info.getValue())}</span>, meta: { align: "right", className: "hidden md:table-cell" } }),
];

export const entryColumns = (actions: EntryActions) => [
  ...fileColumns,
  column.display({
    id: "actions",
    header: () => <span className="sr-only">Actions</span>,
    cell: ({ row }) => (
      <span className="flex justify-end" onClick={(event) => event.stopPropagation()}>
        <EntryRowMenu entry={row.original} {...actions} />
      </span>
    ),
    meta: { align: "right" },
  }),
];
