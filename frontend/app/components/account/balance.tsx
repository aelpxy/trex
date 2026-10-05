import { useSuspenseQuery } from "@tanstack/react-query";

import { tokens } from "~/components/admin/format";
import { Meter } from "~/components/ui/meter";
import { Pagination, usePage } from "~/components/ui/pagination";
import { formatUsd } from "~/lib/credits";
import { LEDGER_PAGE_SIZE, queries } from "~/lib/queries";

const KIND_LABEL: Record<string, string> = { grant: "Grant", usage: "Usage", adjustment: "Adjustment" };
const date = (seconds: number) => new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });

export function BalanceCard() {
  const { data: credits } = useSuspenseQuery(queries.credits());
  const allowance = credits.plan?.monthly_credits ?? 0;
  const left = allowance > 0 ? Math.round(Math.max(0, Math.min(1, credits.balance / allowance)) * 100) : 0;
  return (
    <div className="ui-card p-5">
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

const month = (seconds: number) => new Date(seconds * 1000).toLocaleDateString(undefined, { month: "long" });

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
    <div className="ui-card p-5">
      <p className="text-xs text-muted">Since {month(usage.period_start)} 1</p>
      <dl className="mt-3 grid grid-cols-2 gap-x-4 gap-y-3 sm:grid-cols-4">
        {stats.map((stat) => (
          <div key={stat.label} className="min-w-0">
            <dt className="text-[11px] text-muted">{stat.label}</dt>
            <dd className="mt-0.5 truncate text-base font-medium tabular-nums">{stat.value}</dd>
          </div>
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

export function Ledger() {
  const page = usePage();
  const { data } = useSuspenseQuery(queries.ledger(page));
  const entries = data.data;

  if (data.total_count === 0) return <p className="text-sm text-muted">No activity yet.</p>;
  return (
    <div>
      <div className="ui-card overflow-hidden">
        <table className="w-full text-left text-xs">
          <thead className="border-b border-line text-muted">
            <tr>
              <th className="px-4 py-2.5 font-medium">Date</th>
              <th className="px-4 py-2.5 font-medium">Description</th>
              <th className="px-4 py-2.5 text-right font-medium">Amount</th>
              <th className="px-4 py-2.5 text-right font-medium">Balance</th>
            </tr>
          </thead>
          <tbody>
            {entries.map((entry) => (
              <tr key={entry.id} className="border-b border-line last:border-0">
                <td className="px-4 py-2.5 whitespace-nowrap text-muted">{date(entry.created_at)}</td>
                <td className="px-4 py-2.5">
                  {entry.description}
                  <span className="ml-1.5 text-muted">· {KIND_LABEL[entry.kind] ?? entry.kind}</span>
                </td>
                <td className={`px-4 py-2.5 text-right tabular-nums ${entry.amount > 0 ? "text-ink" : "text-muted"}`}>
                  {entry.amount > 0 ? "+" : ""}
                  {formatUsd(entry.amount)}
                </td>
                <td className="px-4 py-2.5 text-right tabular-nums">{formatUsd(entry.balance)}</td>
              </tr>
            ))}
            {entries.length === 0 && (
              <tr>
                <td colSpan={4} className="px-4 py-4 text-center text-muted">
                  This page is empty.
                </td>
              </tr>
            )}
          </tbody>
        </table>
      </div>
      <Pagination page={page} perPage={LEDGER_PAGE_SIZE} total={data.total_count} />
    </div>
  );
}
