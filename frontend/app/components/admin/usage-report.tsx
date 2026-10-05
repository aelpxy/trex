import { useSuspenseQuery } from "@tanstack/react-query";
import { Link } from "react-router";

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

function Table({ title, headings, rows }: { title: string; headings: string[]; rows: (string | number)[][] }) {
  return (
    <section>
      <h2 className="text-sm font-medium">{title}</h2>
      <table className="ui-card mt-3 w-full overflow-hidden text-left text-xs">
        <thead className="border-b border-line text-muted">
          <tr>
            {headings.map((heading, index) => (
              <th key={heading} className={`px-4 py-2.5 font-medium ${index > 0 ? "text-right" : ""}`}>
                {heading}
              </th>
            ))}
          </tr>
        </thead>
        <tbody>
          {rows.map((row) => (
            <tr key={String(row[0])} className="border-b border-line last:border-0">
              {row.map((cell, index) => (
                <td key={index} className={`px-4 py-2.5 ${index > 0 ? "text-right tabular-nums" : ""}`}>
                  {cell}
                </td>
              ))}
            </tr>
          ))}
          {rows.length === 0 && (
            <tr>
              <td colSpan={headings.length} className="px-4 py-4 text-center text-muted">
                No usage in this range.
              </td>
            </tr>
          )}
        </tbody>
      </table>
    </section>
  );
}

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
      <Table
        title="By model"
        headings={["Model", "Responses", "Input", "Output", "Spend"]}
        rows={report.models.map((model) => [model.model, count(model.responses), tokens(model.input_tokens), tokens(model.output_tokens), formatUsd(model.credits)])}
      />
      <Table
        title="Top workspaces"
        headings={["Workspace", "Responses", "Tokens", "Spend"]}
        rows={report.workspaces.map((workspace) => [workspace.name, count(workspace.responses), tokens(workspace.tokens), formatUsd(workspace.credits)])}
      />
    </div>
  );
}
