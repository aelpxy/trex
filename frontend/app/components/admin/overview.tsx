import { useSuspenseQuery } from "@tanstack/react-query";

import { formatUsd } from "~/lib/credits";
import { queries } from "~/lib/queries";

import { count, tokens } from "./format";

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
        {overview.top_models.length === 0 ? (
          <p className="mt-2 text-sm text-muted">No usage yet.</p>
        ) : (
          <ul className="ui-card mt-3 divide-y divide-line overflow-hidden">
            {overview.top_models.map((model) => (
              <li key={model.model} className="flex items-center gap-4 px-4 py-2.5 text-sm">
                <span className="min-w-0 flex-1 truncate font-mono text-[13px]">{model.model}</span>
                <span className="text-xs text-muted tabular-nums">{tokens(model.input_tokens + model.output_tokens)} tokens</span>
                <span className="w-20 text-right tabular-nums">{formatUsd(model.credits)}</span>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
