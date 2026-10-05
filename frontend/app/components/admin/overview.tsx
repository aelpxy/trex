import { useSuspenseQuery } from "@tanstack/react-query";

import { columnsFor, DataTable } from "~/components/ui/data-table";
import { Stat } from "~/components/ui/stat";
import { formatUsd } from "~/lib/credits";
import { count, tokens } from "~/lib/format";
import { queries } from "~/lib/queries";
import type { ApiOverview } from "~/lib/trex";

const card = "ui-card px-5 py-4";

const column = columnsFor<ApiOverview["top_models"][number]>();
const COLUMNS = [
  column.accessor("model", { header: "Model", cell: (info) => <span className="font-mono text-[13px]">{info.getValue()}</span> }),
  column.accessor((row) => row.input_tokens + row.output_tokens, { id: "tokens", header: "Tokens", cell: (info) => tokens(info.getValue()), meta: { align: "right" } }),
  column.accessor("credits", { header: "Spend", cell: (info) => formatUsd(info.getValue()), meta: { align: "right" } }),
];

export function Overview() {
  const { data: overview } = useSuspenseQuery(queries.admin.overview());
  return (
    <div className="space-y-8">
      <dl className="grid grid-cols-2 gap-3 sm:grid-cols-3">
        <Stat size="lg" className={card} label="Users" value={count(overview.users)} note={`${count(overview.admins)} ${overview.admins === 1 ? "admin" : "admins"}`} />
        <Stat size="lg" className={card} label="Workspaces" value={count(overview.workspaces)} />
        <Stat size="lg" className={card} label="Chats" value={count(overview.chats)} />
        <Stat size="lg" className={card} label="Running now" value={count(overview.running)} note="Agents working at this moment" />
        <Stat size="lg" className={card} label="Spend today" value={formatUsd(overview.spend_today)} note={`${tokens(overview.tokens_today)} tokens`} />
        <Stat size="lg" className={card} label="Spend, last 30 days" value={formatUsd(overview.spend_month)} note={`${tokens(overview.tokens_month)} tokens`} />
      </dl>
      <section>
        <h2 className="text-sm font-medium">Top models, last 30 days</h2>
        <DataTable label="Top models" data={overview.top_models} columns={COLUMNS} rowId={(row) => row.model} empty="No usage yet." />
      </section>
    </div>
  );
}
