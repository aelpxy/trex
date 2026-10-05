import { useCallback, useState } from "react";
import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { LuFolderOpen } from "react-icons/lu";

import { columnsFor, DataTable } from "~/components/ui/data-table";
import { Pagination, usePage } from "~/components/ui/pagination";
import { DrawerStats, SideDrawer } from "~/components/ui/side-drawer";
import { focusRing } from "~/components/ui/styles";
import { formatUsd } from "~/lib/credits";
import { ADMIN_PAGE_SIZE, queries } from "~/lib/queries";
import type { ApiAdminWorkspace } from "~/lib/trex";

import { FilterInput } from "./filter-input";
import { date } from "./format";
import { useDrawerRecord } from "./use-drawer-record";
import { useUrlFilter, useUrlSort } from "./use-url-filter";
import { WorkspaceManager } from "./workspace-manager";

const column = columnsFor<ApiAdminWorkspace>();

export function WorkspaceList() {
  const page = usePage();
  const [filter, setFilter] = useUrlFilter();
  const { sort, sorting, setSorting } = useUrlSort();
  const { data } = useSuspenseQuery(queries.admin.workspaces(page, filter.trim(), sort));
  const { data: plans } = useSuspenseQuery(queries.plans());
  const [selected, setSelected] = useState<ApiAdminWorkspace | null>(null);
  const [open, setOpen] = useState(false);
  const lookup = useQuery({ ...queries.admin.workspaces(1, selected?.id ?? ""), enabled: selected !== null });
  const close = useCallback(() => setOpen(false), []);
  const workspace = useDrawerRecord({
    selected,
    rows: data.data,
    fetched: lookup.data?.data,
    settled: lookup.isSuccess && !lookup.isFetching,
    open,
    onGone: close,
    gone: "That workspace was deleted",
  });
  const planName = (id: string) => plans.find((plan) => plan.id === id)?.name ?? id;

  const columns = [
    column.accessor("name", {
      header: "Workspace",
      cell: ({ row }) => (
        <span className="block min-w-0">
          <span className="block truncate font-medium">{row.original.name}</span>
          <span className="block truncate text-xs text-muted">{row.original.owner_email ?? row.original.id}</span>
        </span>
      ),
    }),
    column.accessor("plan", {
      header: "Plan",
      cell: (info) => <span className="text-xs text-muted">{planName(info.getValue())}</span>,
      meta: { className: "hidden sm:table-cell" },
    }),
    column.accessor("created_at", {
      header: "Created",
      cell: (info) => <span className="text-xs text-muted">{date(info.getValue())}</span>,
      meta: { className: "hidden md:table-cell" },
    }),
    column.accessor("credits", { header: "Balance", cell: (info) => formatUsd(info.getValue()), meta: { align: "right" } }),
  ];

  return (
    <div>
      <FilterInput value={filter} onChange={setFilter} label="Search by name, member email or id" />
      <DataTable
        label="Workspaces"
        data={data.data}
        columns={columns}
        rowId={(row) => row.id}
        sorting={sorting}
        onSortingChange={setSorting}
        onRowClick={(row) => {
          setSelected(row);
          setOpen(true);
        }}
        empty={filter ? "Nothing matches that search." : "No workspaces yet."}
      />
      {(data.data.length > 0 || page > 1) && <Pagination page={page} perPage={ADMIN_PAGE_SIZE} total={data.total_count} noun="workspaces" />}
      {workspace && (
        <SideDrawer
          open={open}
          onOpenChange={setOpen}
          title={workspace.name}
          description={workspace.owner_email ?? workspace.id}
          footer={
            <Link to={`/admin/library?workspace=${workspace.id}`} className={`inline-flex h-8 items-center gap-2 rounded-md px-3 text-xs text-muted transition-colors hover:bg-subtle hover:text-ink ${focusRing}`}>
              <LuFolderOpen size={14} />
              Browse library
            </Link>
          }
        >
          <DrawerStats
            stats={[
              { label: "Balance", value: formatUsd(workspace.credits) },
              { label: "Plan", value: planName(workspace.plan) },
              { label: "Models", value: workspace.allowed_models ? `${workspace.allowed_models.length} allowed` : "All" },
              { label: "Created", value: date(workspace.created_at) },
            ]}
          />
          <WorkspaceManager key={workspace.id} workspace={workspace} />
        </SideDrawer>
      )}
    </div>
  );
}
