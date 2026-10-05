import { LuMonitor, LuSmartphone, LuTablet } from "react-icons/lu";
import { UAParser } from "ua-parser-js";

import { Button } from "~/components/ui/button";
import type { ApiSignInSession } from "~/lib/trex";

const ago = (seconds: number) => {
  const minutes = Math.round((Date.now() / 1000 - seconds) / 60);
  if (minutes < 2) return "Active now";
  if (minutes < 60) return `Active ${minutes}m ago`;
  if (minutes < 60 * 24) return `Active ${Math.round(minutes / 60)}h ago`;
  return `Active ${Math.round(minutes / 60 / 24)}d ago`;
};

const date = (seconds: number) => new Date(seconds * 1000).toLocaleDateString(undefined, { dateStyle: "medium" });

// "Chrome on Windows" and the matching icon, from the stored user agent
function describe(userAgent: string | null) {
  if (!userAgent) return { label: "Unknown device", Icon: LuMonitor };
  const { browser, os, device } = UAParser(userAgent);
  const name = [browser.name, os.name].filter(Boolean).join(" on ") || "Unknown browser";
  const Icon = device.type === "mobile" ? LuSmartphone : device.type === "tablet" ? LuTablet : LuMonitor;
  return { label: name, Icon };
}

type SessionListProps = {
  sessions: ApiSignInSession[];
  onEnd: (session: ApiSignInSession) => void;
  // the session being signed out, if any
  ending?: string | null;
};

export function SessionList({ sessions, onEnd, ending = null }: SessionListProps) {
  if (sessions.length === 0) return <p className="text-sm text-muted">No signed-in devices.</p>;
  return (
    <ul className="ui-card divide-y divide-line overflow-hidden">
      {sessions.map((session) => {
        const { label, Icon } = describe(session.user_agent);
        const ip = session.last_ip ?? session.ip;
        return (
          <li key={session.id} className="flex items-center gap-3 px-4 py-3">
            <Icon size={18} className="shrink-0 text-muted" />
            <div className="min-w-0 flex-1">
              <p className="truncate text-sm">
                {label}
                {session.current && <span className="ml-2 rounded bg-subtle px-1.5 py-0.5 text-[10px] font-medium text-muted">This device</span>}
              </p>
              <p className="truncate text-xs text-muted">
                {session.current ? "Active now" : ago(session.last_used_at)}
                {ip && ` · ${ip}`}
                {` · Signed in ${date(session.created_at)}`}
              </p>
            </div>
            <Button variant="quiet" onClick={() => onEnd(session)} disabled={ending === session.id} className="h-8 shrink-0 px-3 text-xs">
              {ending === session.id ? "Signing out…" : "Sign out"}
            </Button>
          </li>
        );
      })}
    </ul>
  );
}
