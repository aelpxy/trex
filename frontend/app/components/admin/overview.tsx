import { useSuspenseQuery } from "@tanstack/react-query";

import { columnsFor, DataTable } from "~/components/ui/data-table";
import { formatUsd } from "~/lib/credits";
import { queries } from "~/lib/queries";
import type { ApiOverview } from "~/lib/trex";

import { count, tokens } from "./format";

const column = columnsFor<ApiOverview["top_models"][number]>();
const COLUMNS = [
  column.accessor("model", { header: "Model", cell: (info) => <span className="font-mono text-[13px]">{info.getValue()}</span> }),
  column.accessor((row) => row.input_tokens + row.output_tokens, { id: "tokens", header: "Tokens", cell: (info) => tokens(info.getValue()), meta: { align: "right" } }),
  column.accessor("credits", { header: "Spend", cell: (info) => formatUsd(info.getValue()), meta: { align: "right" } }),
];

function Stat({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div className="ui-card px-4 py-3.5">
      <p className="text-[11px] text-muted">{label}</p>
      <p className="mt-1 text-xl font-medium tracking-tight tabular-nums">{value}</p>
      {note && <p className="mt-0.5 text-[11px] text-muted">{note}</p>}
    </div>
  );
}

export function Overview() {
  const { data: overview } = useSuspenseQuery(queries.admin.overview());
  return (
    <div className="space-y-8">
      <div className="grid grid-cols-2 gap-3 sm:grid-cols-3">
        <Stat label="Users" value={count(overview.users)} note={`${count(overview.admins)} ${overview.admins === 1 ? "admin" : "admins"}`} />
        <Stat label="Workspaces" value={count(overview.workspaces)} />
        <Stat label="Chats" value={count(overview.chats)} note={`${count(overview.running)} running now`} />
        <Stat label="Spend today" value={formatUsd(overview.spend_today)} note={`${tokens(overview.tokens_today)} tokens`} />
        <Stat label="Spend, last 30 days" value={formatUsd(overview.spend_month)} note={`${tokens(overview.tokens_month)} tokens`} />
      </div>
      <section>
        <h2 className="text-sm font-medium">Top models, last 30 days</h2>
        <DataTable label="Top models" data={overview.top_models} columns={COLUMNS} rowId={(row) => row.model} empty="No usage yet." />
      </section>
    </div>
  );
}
