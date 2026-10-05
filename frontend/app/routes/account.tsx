import { useState, type FormEvent, type ReactNode } from "react";
import { useRevalidator } from "react-router";

import { Button } from "~/components/ui/button";
import { Page } from "~/components/ui/page";
import { focusRing } from "~/components/ui/styles";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { formatUsd } from "~/lib/credits";
import { pageTitle } from "~/lib/meta";
import { trex, type ApiLedgerEntry } from "~/lib/trex";

import type { Route } from "./+types/account";

export const meta = () => pageTitle("Account");

export async function clientLoader() {
  const [credits, ledger] = await Promise.all([trex.credits(), trex.ledger()]);
  return { credits, ledger };
}

const MIN_PASSWORD_LENGTH = 8;
const errorText = (cause: unknown) => (cause instanceof Error ? cause.message : String(cause));
const date = (seconds: number) => new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });

function Section({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return (
    <section className="mt-10">
      <h2 className="text-sm font-medium">{title}</h2>
      {description && <p className="mt-1 text-xs text-muted">{description}</p>}
      <div className="mt-4">{children}</div>
    </section>
  );
}

function Status({ error, saved }: { error: string | null; saved: string | null }) {
  if (error) return <p role="alert" className="text-xs text-danger">{error}</p>;
  if (saved) return <p role="status" className="text-xs text-muted">{saved}</p>;
  return null;
}

function ProfileForm() {
  const { profile } = useWorkspace();
  const revalidator = useRevalidator();
  const [name, setName] = useState(profile.name);
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);

  async function save(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setSaved(null);
    try {
      await trex.updateMe({ name: name.trim() });
      setSaved("Saved");
      await revalidator.revalidate();
    } catch (cause) {
      setError(errorText(cause));
    }
  }

  return (
    <form onSubmit={save} className="space-y-3">
      <label className="block">
        <span className="mb-1.5 block text-xs font-medium text-muted">Name</span>
        <input value={name} onChange={(event) => setName(event.target.value)} required className={`ui-input ${focusRing}`} />
      </label>
      <label className="block">
        <span className="mb-1.5 block text-xs font-medium text-muted">Email</span>
        <input value={profile.email} disabled className="ui-input" />
      </label>
      <div className="flex items-center gap-3">
        <Button type="submit" disabled={!name.trim() || name.trim() === profile.name}>Save</Button>
        <Status error={error} saved={saved} />
      </div>
    </form>
  );
}

function PasswordForm() {
  const [current, setCurrent] = useState("");
  const [next, setNext] = useState("");
  const [error, setError] = useState<string | null>(null);
  const [saved, setSaved] = useState<string | null>(null);

  async function save(event: FormEvent) {
    event.preventDefault();
    setError(null);
    setSaved(null);
    try {
      await trex.changePassword({ current_password: current, new_password: next });
      setCurrent("");
      setNext("");
      setSaved("Password changed. Other devices were signed out.");
    } catch (cause) {
      setError(errorText(cause));
    }
  }

  return (
    <form onSubmit={save} className="space-y-3">
      <label className="block">
        <span className="mb-1.5 block text-xs font-medium text-muted">Current password</span>
        <input type="password" autoComplete="current-password" value={current} onChange={(event) => setCurrent(event.target.value)} required className={`ui-input ${focusRing}`} />
      </label>
      <label className="block">
        <span className="mb-1.5 block text-xs font-medium text-muted">New password</span>
        <input
          type="password"
          autoComplete="new-password"
          minLength={MIN_PASSWORD_LENGTH}
          value={next}
          onChange={(event) => setNext(event.target.value)}
          required
          className={`ui-input ${focusRing}`}
        />
      </label>
      <div className="flex items-center gap-3">
        <Button type="submit" disabled={!current || next.length < MIN_PASSWORD_LENGTH}>Change password</Button>
        <Status error={error} saved={saved} />
      </div>
    </form>
  );
}

const KIND_LABEL = { grant: "Grant", usage: "Usage", adjustment: "Adjustment" };

function Ledger({ initial, hasMore: initialHasMore }: { initial: ApiLedgerEntry[]; hasMore: boolean }) {
  const [entries, setEntries] = useState(initial);
  const [hasMore, setHasMore] = useState(initialHasMore);
  const [loading, setLoading] = useState(false);

  async function more() {
    setLoading(true);
    try {
      const page = await trex.ledger(entries.at(-1)?.id);
      setEntries((current) => [...current, ...page.data]);
      setHasMore(page.has_more);
    } finally {
      setLoading(false);
    }
  }

  if (entries.length === 0) return <p className="text-sm text-muted">No activity yet.</p>;
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
          </tbody>
        </table>
      </div>
      {hasMore && (
        <div className="mt-3 flex justify-center">
          <Button variant="quiet" onClick={() => void more()} disabled={loading}>
            {loading ? "Loading…" : "Show older"}
          </Button>
        </div>
      )}
    </div>
  );
}

export default function Account({ loaderData }: Route.ComponentProps) {
  const { credits, ledger } = loaderData;
  const allowance = credits.plan?.monthly_credits ?? 0;
  const share = allowance > 0 ? Math.max(0, Math.min(1, credits.balance / allowance)) : 0;

  return (
    <Page title="Account">
      <Section title="Balance" description={credits.enforced ? "Each model response is charged by its tokens. Your plan tops the balance up once a month." : "Credits are tracked but not enforced on this server."}>
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
      </Section>
      <Section title="Activity">
        <Ledger initial={ledger.data} hasMore={ledger.has_more} />
      </Section>
      <Section title="Profile">
        <ProfileForm />
      </Section>
      <Section title="Password" description={`At least ${MIN_PASSWORD_LENGTH} characters. Changing it signs out your other devices.`}>
        <PasswordForm />
      </Section>
    </Page>
  );
}
