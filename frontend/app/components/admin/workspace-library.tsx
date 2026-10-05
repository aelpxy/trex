import { useState } from "react";
import { useSuspenseQuery } from "@tanstack/react-query";
import { Link, useNavigate, useSearchParams } from "react-router";
import { LuArrowLeft, LuDownload, LuExternalLink, LuFolder, LuFolderOpen } from "react-icons/lu";

import { Breadcrumbs } from "~/components/library/breadcrumbs";
import { entriesOf, entryIcon, formatSize, openFile, viewable, type Entry } from "~/components/library/entries";
import { Button } from "~/components/ui/button";
import { columnsFor, DataTable } from "~/components/ui/data-table";
import { EmptyState } from "~/components/ui/empty-state";
import { Pagination, usePage } from "~/components/ui/pagination";
import { focusRing, iconButton } from "~/components/ui/styles";
import { adminLibraryFile } from "~/lib/api";
import { ADMIN_PAGE_SIZE, queries } from "~/lib/queries";
import type { ApiAdminWorkspace } from "~/lib/trex";

import { ActionStatus } from "./action-status";
import { FilterInput } from "./filter-input";
import { date } from "./format";
import { useUrlFilter, useUrlSort } from "./use-url-filter";

const workspaceColumn = columnsFor<ApiAdminWorkspace>();
const WORKSPACE_COLUMNS = [
  workspaceColumn.accessor("name", {
    header: "Workspace",
    cell: ({ row }) => (
      <span className="flex min-w-0 items-center gap-3">
        <LuFolder size={16} className="shrink-0 text-muted" />
        <span className="min-w-0">
          <span className="block truncate font-medium">{row.original.name}</span>
          <span className="block truncate text-xs text-muted">{row.original.owner_email ?? row.original.id}</span>
        </span>
      </span>
    ),
  }),
  workspaceColumn.accessor("created_at", { header: "Created", cell: (info) => <span className="text-xs text-muted">{date(info.getValue())}</span>, meta: { align: "right", className: "hidden sm:table-cell" } }),
];

// step one: which workspace's library to open
function WorkspacePicker() {
  const page = usePage();
  const [filter, setFilter] = useUrlFilter();
  const { sort, sorting, setSorting } = useUrlSort();
  const { data } = useSuspenseQuery(queries.admin.workspaces(page, filter.trim(), sort));
  const [, setParams] = useSearchParams();

  return (
    <div>
      <p className="mb-3 text-sm text-muted">Pick a workspace to browse the files saved to its library.</p>
      <FilterInput value={filter} onChange={setFilter} label="Search by name, member email or id" />
      <DataTable
        label="Workspaces"
        data={data.data}
        columns={WORKSPACE_COLUMNS}
        rowId={(row) => row.id}
        sorting={sorting}
        onSortingChange={setSorting}
        onRowClick={(row) => setParams({ workspace: row.id }, { preventScrollReset: true })}
        empty={filter ? "No workspace matches that search." : "No workspaces yet."}
      />
      {(data.data.length > 0 || page > 1) && <Pagination page={page} perPage={ADMIN_PAGE_SIZE} total={data.total_count} noun="workspaces" />}
    </div>
  );
}

// step two: one workspace's files, a folder at a time
function Browser({ id }: { id: string }) {
  const { data: matches } = useSuspenseQuery(queries.admin.workspaces(1, id));
  const { data: files } = useSuspenseQuery(queries.admin.library(id));
  const [params, setParams] = useSearchParams();
  const [filter, setFilter] = useUrlFilter();
  const [error, setError] = useState<unknown>(null);
  const navigate = useNavigate();
  const workspace = matches.data.find((candidate) => candidate.id === id);
  const folder = params.get("path") ?? "";
  const entries = entriesOf(files, folder, filter);
  const total = files.reduce((sum, file) => sum + file.size, 0);

  const openFolder = (path: string) =>
    setParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (path) next.set("path", path);
        else next.delete("path");
        next.delete("q");
        return next;
      },
      { preventScrollReset: true },
    );

  function fetchFile(file: Entry, download: boolean) {
    setError(null);
    openFile(() => adminLibraryFile(id, file.path), file.path, download).catch(setError);
  }

  const column = columnsFor<Entry>();
  const columns = [
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
              {row.original.kind === "folder" && <span className="block text-xs text-muted">{row.original.files === 1 ? "1 file" : `${row.original.files.toLocaleString()} files`}</span>}
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
      cell: ({ row }) =>
        row.original.kind === "file" && (
          <span className="flex justify-end" onClick={(event) => event.stopPropagation()}>
            {viewable(row.original.name) && (
              <button type="button" onClick={() => fetchFile(row.original, false)} aria-label={`Open ${row.original.name}`} title="Open in a new tab" className={iconButton}>
                <LuExternalLink size={14} />
              </button>
            )}
            <button type="button" onClick={() => fetchFile(row.original, true)} aria-label={`Download ${row.original.name}`} title="Download" className={iconButton}>
              <LuDownload size={14} />
            </button>
          </span>
        ),
      meta: { align: "right" },
    }),
  ];

  return (
    <div>
      <Link to="/admin/library" className={`inline-flex items-center gap-1.5 rounded-md text-xs text-muted hover:text-ink ${focusRing}`}>
        <LuArrowLeft size={13} />
        All workspaces
      </Link>
      <div className="ui-card mt-3 flex items-center gap-3 px-4 py-3.5">
        <LuFolderOpen size={20} className="shrink-0 text-muted" />
        <div className="min-w-0 flex-1">
          <p className="truncate text-sm font-medium">{workspace?.name ?? "Workspace"}</p>
          <p className="truncate text-xs text-muted">{workspace?.owner_email ?? id}</p>
        </div>
        <p className="shrink-0 text-right text-xs text-muted tabular-nums">
          {files.length === 1 ? "1 file" : `${files.length.toLocaleString()} files`}
          <span className="block">{formatSize(total)}</span>
        </p>
      </div>
      {files.length === 0 ? (
        <EmptyState icon={LuFolderOpen} title="Nothing saved yet" description="Files the agent or the user save to this workspace's library show up here." action={<Button variant="quiet" onClick={() => navigate("/admin/library")}>Pick another workspace</Button>} />
      ) : (
        <>
          <div className="mt-4">
            <FilterInput value={filter} onChange={setFilter} label="Search every file in this library" />
          </div>
          <div className="mt-3 flex min-h-6 items-center">{filter ? <p className="text-xs text-muted">{entries.length === 1 ? "1 match" : `${entries.length.toLocaleString()} matches`} across all folders</p> : <Breadcrumbs folder={folder} onOpen={openFolder} />}</div>
          <ActionStatus error={error} success={null} />
          <DataTable
            label="Files"
            data={entries}
            columns={columns}
            rowId={(row) => `${row.kind}:${row.path}`}
            onRowClick={(row) => (row.kind === "folder" ? openFolder(row.path) : fetchFile(row, !viewable(row.name)))}
            empty={filter ? "No file matches that search." : "This folder is empty."}
          />
        </>
      )}
    </div>
  );
}

export function WorkspaceLibrary() {
  const [params] = useSearchParams();
  const workspace = params.get("workspace");
  return workspace ? <Browser key={workspace} id={workspace} /> : <WorkspacePicker />;
}
