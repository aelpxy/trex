import { useQuery, useSuspenseQuery } from "@tanstack/react-query";

import { SessionList } from "~/components/account/session-list";

import { Button } from "~/components/ui/button";
import { Pagination, usePage } from "~/components/ui/pagination";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { formatUsd } from "~/lib/credits";
import { ADMIN_PAGE_SIZE, queries } from "~/lib/queries";
import type { ApiAdminUser } from "~/lib/trex";

import { ActionStatus } from "./action-status";
import { ExpandableRow } from "./expandable-row";
import { FilterInput } from "./filter-input";
import { ago, date } from "./format";
import { useEndUserSession, useSetRole, useSignOutUser } from "./mutations";
import { useUrlFilter } from "./use-url-filter";
import { WorkspaceManager } from "./workspace-manager";

// a user's signed-in browsers, loaded when their row opens
function UserDevices({ user }: { user: ApiAdminUser }) {
  const sessions = useQuery(queries.admin.signInSessions(user.id));
  const end = useEndUserSession();
  return (
    <div className="border-t border-line px-4 py-3">
      <p className="mb-2 text-xs font-medium">Devices</p>
      {sessions.isPending ? (
        <p className="text-xs text-muted">Loading…</p>
      ) : sessions.error ? (
        <ActionStatus error={sessions.error} success={null} />
      ) : (
        <SessionList sessions={sessions.data} onEnd={(session) => end.mutate({ user: user.id, session: session.id })} ending={end.isPending ? end.variables.session : null} />
      )}
      <div className="mt-2">
        <ActionStatus error={end.error} success={null} />
      </div>
    </div>
  );
}

function UserActions({ user }: { user: ApiAdminUser }) {
  const { profile } = useWorkspace();
  const setRole = useSetRole();
  const signOut = useSignOutUser();
  const self = user.id === profile.id;
  const promote = user.role !== "admin";

  return (
    <div className="flex flex-wrap items-center gap-2 border-t border-line px-4 py-3">
      <p className="mr-auto text-[11px] text-muted">
        Joined {date(user.created_at)} · {user.workspaces} {user.workspaces === 1 ? "workspace" : "workspaces"}
      </p>
      <ActionStatus
        error={setRole.error ?? signOut.error}
        success={setRole.isSuccess ? (setRole.variables.role === "admin" ? "Now an admin." : "No longer an admin.") : signOut.isSuccess ? "Signed out everywhere." : null}
      />
      <Button
        variant="quiet"
        disabled={self || setRole.isPending}
        title={self ? "You can't change your own role" : undefined}
        onClick={() => setRole.mutate({ user: user.id, role: promote ? "admin" : "user" })}
        className="h-8 px-3 text-xs"
      >
        {promote ? "Make admin" : "Remove admin"}
      </Button>
      <Button variant="quiet" disabled={signOut.isPending} onClick={() => signOut.mutate(user.id)} className="h-8 px-3 text-xs">
        Sign out everywhere
      </Button>
    </div>
  );
}

// the workspaces a user owns, loaded when their row opens
function OwnedWorkspaces({ user }: { user: ApiAdminUser }) {
  const workspaces = useQuery(queries.admin.workspaces(1, user.email));
  if (workspaces.isPending) return <p className="border-t border-line px-4 py-3 text-xs text-muted">Loading workspaces…</p>;
  if (workspaces.error) return <div className="border-t border-line px-4 py-3"><ActionStatus error={workspaces.error} success={null} /></div>;
  return workspaces.data.data.filter((workspace) => workspace.owner_email === user.email).map((workspace) => <WorkspaceManager key={workspace.id} workspace={workspace} />);
}

export function UserList() {
  const page = usePage();
  const [filter, setFilter] = useUrlFilter();
  const { data } = useSuspenseQuery(queries.admin.users(page, filter.trim()));
  const shown = data.data;

  return (
    <div>
      <FilterInput value={filter} onChange={setFilter} label="Search by name or email" />
      {shown.length === 0 ? (
        <p className="mt-6 text-sm text-muted">{filter ? "Nobody matches that filter." : "No one has signed up yet."}</p>
      ) : (
        <ul className="ui-card mt-4 overflow-hidden">
          {shown.map((user) => (
              <ExpandableRow
                key={user.id}
                summary={
                  <>
                    <span className="min-w-0 flex-1">
                      <span className="block truncate font-medium">
                        {user.name}
                        {user.role === "admin" && <span className="ml-2 rounded bg-subtle px-1.5 py-0.5 text-[10px] font-medium text-muted">Admin</span>}
                      </span>
                      <span className="block truncate text-xs text-muted">{user.email}</span>
                    </span>
                    <span className="hidden shrink-0 text-xs text-muted sm:block">Active {ago(user.last_active_at).toLowerCase()}</span>
                    <span className="w-24 shrink-0 text-right tabular-nums">{user.credits === null ? "—" : formatUsd(user.credits)}</span>
                  </>
                }
              >
                <UserActions user={user} />
                <UserDevices user={user} />
                <OwnedWorkspaces user={user} />
              </ExpandableRow>
          ))}
        </ul>
      )}
      {(shown.length > 0 || page > 1) && <Pagination page={page} perPage={ADMIN_PAGE_SIZE} total={data.total_count} noun="users" />}
    </div>
  );
}
