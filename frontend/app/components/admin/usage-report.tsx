import { useSuspenseQuery } from "@tanstack/react-query";
import { Link } from "react-router";

import { columnsFor, DataTable } from "~/components/ui/data-table";
import { focusRing } from "~/components/ui/styles";
import { formatUsd } from "~/lib/credits";
import { queries } from "~/lib/queries";
import type { ApiUsageReport } from "~/lib/trex";

import { count, tokens } from "./format";

export const USAGE_RANGES = [7, 30, 90];
export const DEFAULT_USAGE_DAYS = 30;
const DAY_SECONDS = 24 * 60 * 60;

export const usageDays = (search: URLSearchParams) => {
  const requested = Number(search.get("days"));
  return USAGE_RANGES.includes(requested) ? requested : DEFAULT_USAGE_DAYS;
};

// every day in the range, with zeros for days nothing ran
function filledDays(report: ApiUsageReport) {
  const today = Math.floor(Date.now() / 1000 / DAY_SECONDS) * DAY_SECONDS;
  const byDay = new Map(report.daily.map((day) => [day.day, day]));
  return Array.from({ length: report.days }, (_, index) => {
    const day = today - (report.days - 1 - index) * DAY_SECONDS;
    return byDay.get(day) ?? { day, credits: 0, input_tokens: 0, output_tokens: 0, responses: 0 };
  });
}

function DailyChart({ report }: { report: ApiUsageReport }) {
  const series = filledDays(report);
  // charts tokens when nothing is priced, so free models still show
  const priced = series.some((day) => day.credits > 0);
  const value = (day: (typeof series)[number]) => (priced ? day.credits : day.input_tokens + day.output_tokens);
  const peak = Math.max(1, ...series.map(value));
  return (
    <section>
      <h2 className="text-sm font-medium">{priced ? "Spend" : "Tokens"} per day</h2>
      <div className="ui-card mt-3 flex h-40 items-end gap-px p-3" role="img" aria-label={`${priced ? "Spend" : "Tokens"} per day over the last ${report.days} days`}>
        {series.map((day) => (
          <div
            key={day.day}
            title={`${new Date(day.day * 1000).toLocaleDateString(undefined, { timeZone: "UTC", dateStyle: "medium" })}: ${formatUsd(day.credits)}, ${tokens(day.input_tokens + day.output_tokens)} tokens, ${count(day.responses)} responses`}
            className="min-w-0 flex-1 rounded-t-sm bg-ink/70 transition-colors hover:bg-ink"
            style={{ height: `${Math.max(value(day) > 0 ? 2 : 0, (value(day) / peak) * 100)}%` }}
          />
        ))}
      </div>
    </section>
  );
}

type ModelRow = ApiUsageReport["models"][number];
type AccountRow = ApiUsageReport["accounts"][number];

const modelColumn = columnsFor<ModelRow>();
const MODEL_COLUMNS = [
  modelColumn.accessor("model", { header: "Model", cell: (info) => <span className="font-mono text-[13px]">{info.getValue()}</span> }),
  modelColumn.accessor("responses", { header: "Responses", cell: (info) => count(info.getValue()), meta: { align: "right", className: "hidden sm:table-cell" } }),
  modelColumn.accessor("input_tokens", { header: "Input", cell: (info) => tokens(info.getValue()), meta: { align: "right", className: "hidden sm:table-cell" } }),
  modelColumn.accessor("output_tokens", { header: "Output", cell: (info) => tokens(info.getValue()), meta: { align: "right", className: "hidden sm:table-cell" } }),
  modelColumn.accessor("credits", { header: "Spend", cell: (info) => formatUsd(info.getValue()), meta: { align: "right" } }),
];

const accountColumn = columnsFor<AccountRow>();
const ACCOUNT_COLUMNS = [
  accountColumn.accessor("name", {
    header: "Account",
    cell: ({ row }) => (
      <span className="block min-w-0">
        <span className="block truncate font-medium">{row.original.name}</span>
        <span className="block truncate text-xs text-muted">{row.original.email}</span>
      </span>
    ),
  }),
  accountColumn.accessor("responses", { header: "Responses", cell: (info) => count(info.getValue()), meta: { align: "right", className: "hidden sm:table-cell" } }),
  accountColumn.accessor("tokens", { header: "Tokens", cell: (info) => tokens(info.getValue()), meta: { align: "right" } }),
  accountColumn.accessor("credits", { header: "Spend", cell: (info) => formatUsd(info.getValue()), meta: { align: "right" } }),
];

export function UsageReport({ days }: { days: number }) {
  const { data: report } = useSuspenseQuery(queries.admin.usage(days));
  return (
    <div className="space-y-8">
      <nav aria-label="Range" className="flex items-center gap-1">
        {USAGE_RANGES.map((range) => (
          <Link
            key={range}
            to={`?days=${range}`}
            replace
            preventScrollReset
            aria-current={days === range ? "page" : undefined}
            className={`h-7 rounded-md px-2.5 text-xs leading-7 font-medium text-muted hover:text-ink aria-[current=page]:bg-subtle aria-[current=page]:text-ink ${focusRing}`}
          >
            {range} days
          </Link>
        ))}
      </nav>
      <DailyChart report={report} />
      <section>
        <h2 className="text-sm font-medium">By model</h2>
        <DataTable label="Usage by model" data={report.models} columns={MODEL_COLUMNS} rowId={(row) => row.model} empty="No usage in this range." />
      </section>
      <section>
        <h2 className="text-sm font-medium">Top accounts</h2>
        <DataTable label="Top accounts" data={report.accounts} columns={ACCOUNT_COLUMNS} rowId={(row) => row.user} empty="No usage in this range." />
      </section>
    </div>
  );
}
