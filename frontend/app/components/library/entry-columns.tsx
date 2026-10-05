import { date } from "~/components/admin/format";
import { columnsFor } from "~/components/ui/data-table";

import { entryIcon, formatSize, plural, type Entry } from "./entries";
import { EntryRowMenu, type EntryActions } from "./entry-menu";

const column = columnsFor<Entry>();

export const entryColumns = (actions: EntryActions) => [
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
