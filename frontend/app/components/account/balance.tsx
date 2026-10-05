import { useSuspenseQuery } from "@tanstack/react-query";

import { columnsFor, DataTable } from "~/components/ui/data-table";
import { Meter } from "~/components/ui/meter";
import { Pagination, usePage } from "~/components/ui/pagination";
import { Stat } from "~/components/ui/stat";
import { formatUsd } from "~/lib/credits";
import { dateTime, tokens } from "~/lib/format";
import { LEDGER_PAGE_SIZE, queries } from "~/lib/queries";
import type { ApiLedgerEntry } from "~/lib/trex";

const KIND_LABEL: Record<string, string> = { grant: "Grant", usage: "Usage", adjustment: "Adjustment" };

export function BalanceCard() {
  const { data: credits } = useSuspenseQuery(queries.credits());
  const allowance = credits.plan?.monthly_credits ?? 0;
  const left = allowance > 0 ? Math.round(Math.max(0, Math.min(1, credits.balance / allowance)) * 100) : 0;
  return (
    <div className="ui-card px-5 py-4">
      <div className="flex items-baseline justify-between gap-4">
        <p className="text-3xl font-medium tracking-tight tabular-nums">{formatUsd(credits.balance)}</p>
        <p className="text-xs text-muted">{credits.plan ? `${credits.plan.name} plan · ${formatUsd(allowance)} a month` : "No plan"}</p>
      </div>
      {allowance > 0 && (
        <div className="mt-4">
          <Meter label="Left of this month's allowance" value={credits.balance} max={allowance} detail={`${left}%`} valueText={`${formatUsd(credits.balance)} of ${formatUsd(allowance)} left`} />
        </div>
      )}
    </div>
  );
}

// months are counted in UTC on the server, the same as the plan top-up
const month = (seconds: number) => new Date(seconds * 1000).toLocaleDateString(undefined, { month: "long", timeZone: "UTC" });

// this month's spend, and how it splits across models
export function MonthUsage() {
  const { data: usage } = useSuspenseQuery(queries.monthUsage());
  // prices can be zero, and then tokens show the split instead
  const bySpend = usage.credits > 0;
  const total = bySpend ? usage.credits : usage.input_tokens + usage.output_tokens;
  const stats = [
    { label: "Spent", value: formatUsd(usage.credits) },
    { label: "Responses", value: usage.responses.toLocaleString() },
    { label: "Input tokens", value: tokens(usage.input_tokens) },
    { label: "Output tokens", value: tokens(usage.output_tokens) },
  ];

  return (
    <div className="ui-card px-5 py-4">
      <p className="text-xs text-muted">Since {month(usage.period_start)} 1</p>
      <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-3 sm:grid-cols-4">
        {stats.map((stat) => (
          <Stat key={stat.label} size="lg" label={stat.label} value={stat.value} />
        ))}
      </dl>
      {usage.models.length > 0 ? (
        <div className="mt-5 space-y-3 border-t border-line pt-4">
          {usage.models.map((model) => {
            const value = bySpend ? model.credits : model.input_tokens + model.output_tokens;
            const share = total > 0 ? Math.round((value / total) * 100) : 0;
            return (
              <Meter
                key={model.model}
                label={model.name}
                value={value}
                max={total}
                detail={`${bySpend ? formatUsd(model.credits) : `${tokens(value)} tokens`} · ${share}%`}
                valueText={`${share}% of this month's ${bySpend ? "spend" : "tokens"}`}
              />
            );
          })}
        </div>
      ) : (
        <p className="mt-4 text-sm text-muted">Nothing used yet this month.</p>
      )}
    </div>
  );
}

const column = columnsFor<ApiLedgerEntry>();
// the server pages the ledger newest first, so sorting one page in the browser would mislead
const LEDGER_COLUMNS = [
  column.accessor("created_at", { header: "Date",
    enableSorting: false, cell: (info) => <span className="whitespace-nowrap text-xs text-muted">{dateTime(info.getValue())}</span> }),
  column.accessor("description", {
    header: "Description",
    enableSorting: false,
    cell: ({ row }) => (
      <span className="block min-w-0">
        <span className="block truncate">{row.original.description}</span>
        <span className="block text-xs text-muted">{KIND_LABEL[row.original.kind] ?? row.original.kind}</span>
      </span>
    ),
  }),
  column.accessor("amount", {
    header: "Amount",
    enableSorting: false,
    cell: (info) => <span className={info.getValue() > 0 ? "text-ink" : "text-muted"}>{`${info.getValue() > 0 ? "+" : ""}${formatUsd(info.getValue())}`}</span>,
    meta: { align: "right" },
  }),
  column.accessor("balance", { header: "Balance",
    enableSorting: false, cell: (info) => formatUsd(info.getValue()), meta: { align: "right", className: "hidden sm:table-cell" } }),
];

export function Ledger() {
  const page = usePage();
  const { data } = useSuspenseQuery(queries.ledger(page));

  if (data.total_count === 0) return <p className="text-sm text-muted">No activity yet.</p>;
  return (
    <div>
      <DataTable label="Activity" data={data.data} columns={LEDGER_COLUMNS} rowId={(entry) => entry.id} empty="This page is empty." className="" />
      <Pagination page={page} perPage={LEDGER_PAGE_SIZE} total={data.total_count} />
    </div>
  );
}
