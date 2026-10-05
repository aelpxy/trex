import { useState, type FormEvent } from "react";
import { Field } from "@base-ui/react/field";
import { useQuery, useSuspenseQuery } from "@tanstack/react-query";
import { LuTrash2, LuX } from "react-icons/lu";

import { MIN_PASSWORD_LENGTH } from "~/components/account/password-form";
import { SessionList } from "~/components/account/session-list";
import { PasswordInput } from "~/components/auth/password-input";
import { Button } from "~/components/ui/button";
import { columnsFor, DataTable, type RowSelectionState } from "~/components/ui/data-table";
import { DeleteConfirmDialog } from "~/components/ui/delete-confirm-dialog";
import { Pagination, usePage } from "~/components/ui/pagination";
import { DrawerSection, DrawerStats, SideDrawer } from "~/components/ui/side-drawer";
import { Switch } from "~/components/ui/switch";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { formatUsd } from "~/lib/credits";
import { ADMIN_PAGE_SIZE, queries } from "~/lib/queries";
import { toastOutcome, trackToast } from "~/lib/toasts";
import type { ApiAdminUser } from "~/lib/trex";

import { LoadError } from "./load-error";
import { FilterInput } from "./filter-input";
import { ago, date } from "./format";
import { useDeleteUser, useDeleteUsers, useEndUserSession, useSetRole, useSetSuspended, useSetUserPassword, useSignOutUser } from "./mutations";
import { useUrlFilter, useUrlSort } from "./use-url-filter";
import { WorkspaceManager } from "./workspace-manager";

const badge = "ml-2 rounded bg-subtle px-1.5 py-0.5 text-[10px] font-medium text-muted";
const suspendedBadge = "ml-2 rounded bg-danger/10 px-1.5 py-0.5 text-[10px] font-medium text-danger";

const column = columnsFor<ApiAdminUser>();
const COLUMNS = [
  column.accessor("name", {
    header: "User",
    cell: ({ row }) => (
      <span className="block min-w-0">
        <span className="block truncate font-medium">
          {row.original.name}
          {row.original.role === "admin" && <span className={badge}>Admin</span>}
          {row.original.suspended_at !== null && <span className={suspendedBadge}>Suspended</span>}
        </span>
        <span className="block truncate text-xs text-muted">{row.original.email}</span>
      </span>
    ),
  }),
  column.accessor("last_active_at", {
    header: "Last active",
    cell: (info) => <span className="text-xs text-muted">{ago(info.getValue())}</span>,
    meta: { className: "hidden sm:table-cell" },
  }),
  column.accessor("created_at", {
    header: "Joined",
    cell: (info) => <span className="text-xs text-muted">{date(info.getValue())}</span>,
    meta: { className: "hidden md:table-cell" },
  }),
  column.accessor("credits", {
    header: "Balance",
    cell: (info) => (info.getValue() === null ? "—" : formatUsd(info.getValue() ?? 0)),
    meta: { align: "right" },
  }),
];

// a user's signed-in browsers, which can all be signed out at once
function Devices({ user }: { user: ApiAdminUser }) {
  const sessions = useQuery(queries.admin.signInSessions(user.id));
  const end = useEndUserSession();
  const signOut = useSignOutUser();
  const { profile } = useWorkspace();
  // your own devices are signed out from your account, so this one keeps working
  const signedIn = (sessions.data?.length ?? 0) > 0 && user.id !== profile.id;

  return (
    <DrawerSection
      title="Devices"
      description="Browsers they're signed in on."
      action={
        signedIn && (
          <Button
            variant="quiet"
            disabled={signOut.isPending}
            onClick={() => void toastOutcome(signOut.mutateAsync(user.id), { success: `Signed ${user.name} out everywhere`, error: `Couldn't sign ${user.name} out` })}
            className="h-8 px-3 text-xs"
          >
            Sign out everywhere
          </Button>
        )
      }
    >
      {sessions.isPending ? (
        <p className="text-xs text-muted">Loading…</p>
      ) : sessions.error ? (
        <LoadError error={sessions.error} />
      ) : (
        <SessionList
          sessions={sessions.data}
          onEnd={(session) => void toastOutcome(end.mutateAsync({ user: user.id, session: session.id }), { success: "Device signed out", error: "Couldn't sign that device out" })}
          ending={end.isPending ? end.variables.session : null}
        />
      )}
    </DrawerSection>
  );
}

function Access({ user }: { user: ApiAdminUser }) {
  const { profile } = useWorkspace();
  const setRole = useSetRole();
  const setSuspended = useSetSuspended();
  const self = user.id === profile.id;

  return (
    <DrawerSection title="Access">
      <div className="space-y-4">
        <Switch
          checked={user.role === "admin"}
          disabled={self || setRole.isPending}
          onCheckedChange={(admin) =>
            void toastOutcome(setRole.mutateAsync({ user: user.id, role: admin ? "admin" : "user" }), {
              success: admin ? `${user.name} is now an admin` : `${user.name} is no longer an admin`,
              error: "Couldn't change their role",
            })
          }
          description={self ? "You can't change your own role." : "Admins manage every user, workspace and balance."}
        >
          Admin
        </Switch>
        <Switch
          checked={user.suspended_at !== null}
          disabled={self || setSuspended.isPending}
          onCheckedChange={(suspended) =>
            void toastOutcome(setSuspended.mutateAsync({ user: user.id, suspended }), {
              success: suspended ? `Suspended ${user.name}` : `Lifted ${user.name}'s suspension`,
              error: suspended ? "Couldn't suspend them" : "Couldn't lift the suspension",
            })
          }
          description={
            self
              ? "You can't suspend yourself."
              : user.suspended_at !== null
                ? `Since ${date(user.suspended_at)}. They can't sign in and their scheduled tasks wait; everything else is kept.`
                : "Signs them out, stops their running chats and blocks signing in until you lift it."
          }
        >
          Suspended
        </Switch>
      </div>
    </DrawerSection>
  );
}

// for users locked out of their account; the admin shares the new password with them
function Password({ user }: { user: ApiAdminUser }) {
  const { profile } = useWorkspace();
  const [password, setPassword] = useState("");
  const save = useSetUserPassword();
  if (user.id === profile.id) return null;

  function submit(event: FormEvent) {
    event.preventDefault();
    void toastOutcome(save.mutateAsync({ user: user.id, password }), { success: `Password set; ${user.name} is signed out everywhere`, error: "Couldn't set the password" }).then(
      (saved) => saved && setPassword(""),
    );
  }

  return (
    <DrawerSection title="Password" description={`Sets a new one of at least ${MIN_PASSWORD_LENGTH} characters and signs them out everywhere. Share it with them yourself.`}>
      <form onSubmit={submit} className="flex items-end gap-2">
        <Field.Root className="min-w-0 flex-1">
          <Field.Label className="mb-1.5 block text-[11px] font-medium text-muted">New password</Field.Label>
          <PasswordInput value={password} onValueChange={setPassword} minLength={MIN_PASSWORD_LENGTH} autoComplete="new-password" />
        </Field.Root>
        <Button type="submit" disabled={password.length < MIN_PASSWORD_LENGTH || save.isPending}>
          Set password
        </Button>
      </form>
    </DrawerSection>
  );
}

// the workspaces a user owns, loaded when their drawer opens
function OwnedWorkspaces({ user }: { user: ApiAdminUser }) {
  const workspaces = useQuery(queries.admin.workspaces(1, user.email));
  if (workspaces.isPending) return <DrawerSection title="Workspace"><p className="text-xs text-muted">Loading…</p></DrawerSection>;
  if (workspaces.error) return <DrawerSection title="Workspace"><LoadError error={workspaces.error} /></DrawerSection>;
  return workspaces.data.data.filter((workspace) => workspace.owner_email === user.email).map((workspace) => <WorkspaceManager key={workspace.id} workspace={workspace} />);
}

function DeleteUser({ user, onDeleted }: { user: ApiAdminUser; onDeleted: () => void }) {
  const { profile } = useWorkspace();
  const [confirming, setConfirming] = useState(false);
  const remove = useDeleteUser();
  const self = user.id === profile.id;

  return (
    <>
      <Button variant="subtleDanger" disabled={self || remove.isPending} title={self ? "You can't delete yourself" : undefined} onClick={() => setConfirming(true)} className="h-8 px-3 text-xs">
        <LuTrash2 size={14} />
        Delete user
      </Button>
      <DeleteConfirmDialog
        target={confirming ? { kind: "user", id: user.id, name: user.email } : null}
        consequence=" with their workspace, chats, sandboxes, files, schedules and balance"
        onCancel={() => setConfirming(false)}
        onConfirm={() => {
          setConfirming(false);
          trackToast(remove.mutateAsync(user.id), { loading: `Deleting ${user.email}…`, success: `Deleted ${user.email}`, error: `Couldn't delete ${user.email}` }).then(onDeleted, () => {});
        }}
      />
    </>
  );
}

const plural = (count: number) => (count === 1 ? "1 user" : `${count} users`);

// the users ticked on this page, deleted together after one confirmation
function BulkDelete({ users, onClear }: { users: ApiAdminUser[]; onClear: () => void }) {
  const [confirming, setConfirming] = useState(false);
  const remove = useDeleteUsers();
  const shown = users.slice(0, 3).map((user) => user.email).join(", ");
  const names = users.length > 3 ? `${shown} and ${users.length - 3} more` : shown;

  if (users.length === 0) return null;
  return (
    <div role="toolbar" aria-label="Selected users" className="ui-card mt-3 flex items-center gap-2 px-2 py-1.5">
      <Button variant="quiet" onClick={onClear} aria-label="Clear selection" title="Clear selection" className="h-8 px-2">
        <LuX size={14} />
      </Button>
      <p className="text-xs font-medium tabular-nums">{plural(users.length)} selected</p>
      <span className="flex-1" />
      <Button variant="subtleDanger" disabled={remove.isPending} onClick={() => setConfirming(true)} className="h-8 px-3 text-xs">
        <LuTrash2 size={14} />
        Delete {plural(users.length)}
      </Button>
      <DeleteConfirmDialog
        target={confirming ? { kind: users.length === 1 ? "user" : "users", id: "", name: names } : null}
        consequence={users.length === 1 ? " with their workspace, chats, sandboxes, files, schedules and balance" : " with their workspaces, chats, sandboxes, files, schedules and balances"}
        onCancel={() => setConfirming(false)}
        onConfirm={() => {
          setConfirming(false);
          const what = plural(users.length);
          trackToast(
            remove.mutateAsync(users.map((user) => user.id)),
            { loading: `Deleting ${what}…`, success: `Deleted ${what}`, error: `Couldn't delete every user` },
          ).then(onClear, onClear);
        }}
      />
    </div>
  );
}

export function UserList() {
  const page = usePage();
  const [filter, setFilter] = useUrlFilter();
  const { sort, sorting, setSorting } = useUrlSort();
  const { data } = useSuspenseQuery(queries.admin.users(page, filter.trim(), sort));
  const [selected, setSelected] = useState<ApiAdminUser | null>(null);
  const [open, setOpen] = useState(false);
  // the drawer follows refetches, and keeps the last user while it animates closed
  const user = data.data.find((candidate) => candidate.id === selected?.id) ?? selected;
  const { profile } = useWorkspace();
  // ticks belong to one page, search and sort, so changing any of them clears them
  const view = `${page}|${filter.trim()}|${sort}`;
  const [ticked, setTicked] = useState<{ view: string; rows: RowSelectionState }>({ view, rows: {} });
  const selection = ticked.view === view ? ticked.rows : {};
  const setSelection = (rows: RowSelectionState) => setTicked({ view, rows });

  return (
    <div>
      <FilterInput value={filter} onChange={setFilter} label="Search by name or email" />
      <BulkDelete users={data.data.filter((row) => selection[row.id])} onClear={() => setSelection({})} />
      <DataTable
        label="Users"
        data={data.data}
        columns={COLUMNS}
        rowId={(row) => row.id}
        sorting={sorting}
        onSortingChange={setSorting}
        selection={selection}
        onSelectionChange={setSelection}
        canSelect={(row) => row.id !== profile.id}
        rowLabel={(row) => row.email}
        onRowClick={(row) => {
          setSelected(row);
          setOpen(true);
        }}
        empty={filter ? "Nobody matches that search." : "No one has signed up yet."}
      />
      {(data.data.length > 0 || page > 1) && <Pagination page={page} perPage={ADMIN_PAGE_SIZE} total={data.total_count} noun="users" />}
      {user && (
        <SideDrawer
          open={open}
          onOpenChange={setOpen}
          title={
            <>
              {user.name}
              {user.role === "admin" && <span className={badge}>Admin</span>}
              {user.suspended_at !== null && <span className={suspendedBadge}>Suspended</span>}
            </>
          }
          description={user.email}
          footer={<DeleteUser key={user.id} user={user} onDeleted={() => setOpen(false)} />}
        >
          <DrawerStats
            stats={[
              { label: "Balance", value: user.credits === null ? "—" : formatUsd(user.credits) },
              { label: "Last active", value: ago(user.last_active_at) },
              { label: "Joined", value: date(user.created_at) },
              { label: "Workspaces", value: user.workspaces },
            ]}
          />
          <Access key={`access-${user.id}`} user={user} />
          <Password key={`password-${user.id}`} user={user} />
          <OwnedWorkspaces key={`workspaces-${user.id}`} user={user} />
          <Devices key={`devices-${user.id}`} user={user} />
        </SideDrawer>
      )}
    </div>
  );
}
