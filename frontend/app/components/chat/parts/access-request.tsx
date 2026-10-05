import { LuShieldAlert, LuShieldCheck, LuShieldX } from "react-icons/lu";

import { Button } from "~/components/ui/button";

import type { AccessPart } from "../types";

type AccessRequestProps = { part: AccessPart; onResolve: (approved: boolean) => void };

export function AccessRequest({ part, onResolve }: AccessRequestProps) {
  if (part.state !== "pending") {
    const approved = part.state === "approved";
    const Icon = approved ? LuShieldCheck : LuShieldX;
    return (
      <p className="flex items-center gap-2 text-xs text-muted">
        <Icon size={13} className={approved ? "" : "text-danger"} />
        Network access to <span className="font-medium text-ink">{part.host}</span> {approved ? "approved" : "rejected"}
      </p>
    );
  }

  return (
    <section aria-label="Network access request" className="rounded-xl border border-line bg-surface/60 p-4">
      <div className="flex gap-3">
        <LuShieldAlert size={18} className="mt-0.5 shrink-0 text-muted" />
        <div className="min-w-0">
          <h3 className="text-sm font-medium">Allow network access?</h3>
          <p className="mt-1 text-[13px] text-muted">
            <code className="rounded bg-subtle px-1 font-mono text-[11px]">{part.binary}</code> wants to reach <span className="font-medium text-ink">{part.host}</span>. The sandbox blocks unlisted hosts until you approve them.
          </p>
        </div>
      </div>
      <div className="mt-4 flex justify-end gap-2">
        <Button variant="quiet" onClick={() => onResolve(false)}>
          Reject
        </Button>
        <Button onClick={() => onResolve(true)}>Approve</Button>
      </div>
    </section>
  );
}
