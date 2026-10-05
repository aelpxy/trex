import { useSuspenseQuery } from "@tanstack/react-query";

import { Pagination, usePage } from "~/components/ui/pagination";
import { formatUsd } from "~/lib/credits";
import { LEDGER_PAGE_SIZE, queries } from "~/lib/queries";

const KIND_LABEL: Record<string, string> = { grant: "Grant", usage: "Usage", adjustment: "Adjustment" };
const date = (seconds: number) => new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });

export function BalanceCard() {
  const { data: credits } = useSuspenseQuery(queries.credits());
  const allowance = credits.plan?.monthly_credits ?? 0;
  const share = allowance > 0 ? Math.max(0, Math.min(1, credits.balance / allowance)) : 0;
  return (
    <div className="ui-card p-5">
      <div className="flex items-baseline justify-between gap-4">
        <p className="text-3xl font-medium tracking-tight tabular-nums">{formatUsd(credits.balance)}</p>
        <p className="text-xs text-muted">{credits.plan ? `${credits.plan.name} plan · ${formatUsd(allowance)} a month` : "No plan"}</p>
      </div>
      {allowance > 0 && (
        <div className="mt-4 h-1.5 overflow-hidden rounded-full bg-subtle" role="meter" aria-label="Balance left of this month's allowance" aria-valuemin={0} aria-valuemax={allowance} aria-valuenow={credits.balance}>
          <div className="h-full rounded-full bg-ink transition-[width]" style={{ width: `${share * 100}%` }} />
        </div>
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
