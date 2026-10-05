import { useSuspenseQuery } from "@tanstack/react-query";

import { Pagination, usePage } from "~/components/ui/pagination";
import { formatUsd } from "~/lib/credits";
import { ADMIN_PAGE_SIZE, queries } from "~/lib/queries";

import { ExpandableRow } from "./expandable-row";
import { FilterInput } from "./filter-input";
import { useUrlFilter } from "./use-url-filter";
import { WorkspaceManager } from "./workspace-manager";

export function WorkspaceList() {
  const page = usePage();
  const [filter, setFilter] = useUrlFilter();
  const { data } = useSuspenseQuery(queries.admin.workspaces(page, filter.trim()));
  const { data: plans } = useSuspenseQuery(queries.plans());
  const shown = data.data;
  const planName = (id: string) => plans.find((plan) => plan.id === id)?.name ?? id;

  return (
    <div>
      <FilterInput value={filter} onChange={setFilter} label="Search by name, member email or id" />
      {shown.length === 0 ? (
        <p className="mt-6 text-sm text-muted">{filter ? "Nothing matches that filter." : "No workspaces yet."}</p>
      ) : (
        <ul className="ui-card mt-4 overflow-hidden">
          {shown.map((workspace) => (
            <ExpandableRow
              key={workspace.id}
              summary={
                <>
                  <span className="min-w-0 flex-1">
                    <span className="block truncate font-medium">{workspace.name}</span>
                    <span className="block truncate text-xs text-muted">{workspace.owner_email ?? workspace.id}</span>
                  </span>
                  {workspace.allowed_models && <span className="shrink-0 text-xs text-muted">{workspace.allowed_models.length} models</span>}
                  <span className="shrink-0 text-xs text-muted">{planName(workspace.plan)}</span>
                  <span className="w-24 shrink-0 text-right tabular-nums">{formatUsd(workspace.credits)}</span>
                </>
              }
            >
              <WorkspaceManager workspace={workspace} />
            </ExpandableRow>
          ))}
        </ul>
      )}
      {(shown.length > 0 || page > 1) && <Pagination page={page} perPage={ADMIN_PAGE_SIZE} total={data.total_count} noun="workspaces" />}
    </div>
  );
}
