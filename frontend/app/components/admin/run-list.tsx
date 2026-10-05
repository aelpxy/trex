import { useSuspenseQuery } from "@tanstack/react-query";
import { LuActivity, LuSquare } from "react-icons/lu";

import { Button } from "~/components/ui/button";
import { columnsFor, DataTable } from "~/components/ui/data-table";
import { EmptyState } from "~/components/ui/empty-state";
import { formatUsd } from "~/lib/credits";
import { queries } from "~/lib/queries";
import { trackToast } from "~/lib/toasts";
import type { ApiLiveRun } from "~/lib/trex";

import { useCancelRun } from "./mutations";
import { useNow } from "./use-now";

// a run's instance renews its lease every 10s; past this, another instance resumes it
const STALE_AFTER_SECS = 30;
const badge = "ml-2 rounded px-1.5 py-0.5 text-[10px] font-medium";

function duration(seconds: number) {
  const s = Math.max(0, seconds);
  if (s < 60) return `${s}s`;
  if (s < 3600) return `${Math.floor(s / 60)}m ${s % 60}s`;
  return `${Math.floor(s / 3600)}h ${Math.floor((s % 3600) / 60)}m`;
}

function StopButton({ run }: { run: ApiLiveRun }) {
  const cancel = useCancelRun();
  const title = run.title ?? "Untitled chat";
  return (
    <Button
      variant="subtleDanger"
      disabled={run.cancel_requested || cancel.isPending}
      onClick={() =>
        void trackToast(cancel.mutateAsync(run.session), { loading: `Stopping “${title}”…`, success: `Asked “${title}” to stop; it ends within 10s`, error: `Couldn't stop “${title}”` }).catch(() => {})
      }
      className="h-8 px-3 text-xs"
    >
      <LuSquare size={12} />
      {run.cancel_requested ? "Stopping…" : "Stop"}
    </Button>
  );
}

const column = columnsFor<ApiLiveRun>();

export function RunList() {
  const { data: runs } = useSuspenseQuery(queries.admin.runs());
  const now = useNow();

  const columns = [
    column.accessor("title", {
      header: "Chat",
      cell: ({ row }) => (
        <span className="block min-w-0">
          <span className="block truncate font-medium">
            {row.original.title ?? "Untitled chat"}
            {row.original.scheduled && <span className={`${badge} bg-subtle text-muted`}>Scheduled</span>}
          </span>
          <span className="block truncate text-xs text-muted">{row.original.owner_email ?? row.original.workspace_name}</span>
        </span>
      ),
    }),
    column.accessor("model", {
      header: "Model",
      cell: ({ row }) => (
        <span className="text-xs text-muted">
          <span className="font-mono">{row.original.model}</span>
          {row.original.reasoning_effort && ` · ${row.original.reasoning_effort}`}
          {row.original.fast && " · fast"}
        </span>
      ),
      meta: { className: "hidden md:table-cell" },
    }),
    column.accessor("started_at", {
      header: "Running for",
      cell: (info) => <span className="text-xs tabular-nums">{info.getValue() === null ? "—" : duration(now - (info.getValue() ?? now))}</span>,
      meta: { align: "right" },
    }),
    column.accessor("heartbeat_at", {
      header: "Last check-in",
      cell: (info) => {
        const age = info.getValue() === null ? null : now - (info.getValue() ?? now);
        if (age !== null && age > STALE_AFTER_SECS) {
          return (
            <span className={`${badge} bg-danger/10 text-danger`} title="Its server stopped renewing it; another server resumes it shortly">
              Stalled {duration(age)}
            </span>
          );
        }
        return <span className="text-xs text-muted tabular-nums">{age === null ? "—" : `${duration(age)} ago`}</span>;
      },
      meta: { align: "right", className: "hidden sm:table-cell" },
    }),
    column.accessor("credits", {
      header: "Spent",
      cell: ({ row }) => (
        <span className="block">
          {formatUsd(row.original.credits)}
          <span className="block text-xs text-muted">{row.original.responses === 1 ? "1 response" : `${row.original.responses} responses`}</span>
        </span>
      ),
      meta: { align: "right", className: "hidden sm:table-cell" },
    }),
    column.display({ id: "stop", header: () => <span className="sr-only">Stop</span>, cell: ({ row }) => <StopButton run={row.original} />, meta: { align: "right" } }),
  ];

  if (runs.length === 0) {
    return <EmptyState icon={LuActivity} title="Nothing is running" description="Chats whose agent is working show up here as they start, across every workspace." />;
  }
  return (
    <div>
      <p className="text-sm text-muted">
        {runs.length === 1 ? "1 run" : `${runs.length} runs`} in progress, longest first. Refreshes every few seconds.
      </p>
      <DataTable label="Live runs" data={runs} columns={columns} rowId={(run) => run.session} />
    </div>
  );
}
