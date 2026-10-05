import { useState } from "react";
import { Menu } from "@base-ui/react/menu";
import { useSuspenseQuery } from "@tanstack/react-query";
import { Link, useSearchParams } from "react-router";
import { LuBox, LuEllipsis, LuSquare, LuTrash2 } from "react-icons/lu";

import { Button } from "~/components/ui/button";
import { columnsFor, DataTable } from "~/components/ui/data-table";
import { DeleteConfirmDialog, type DeleteTarget } from "~/components/ui/delete-confirm-dialog";
import { EmptyState } from "~/components/ui/empty-state";
import { badge, dangerBadge, dangerMenuItem, iconButton, menuItem, menuSeparator, popup } from "~/components/ui/styles";
import { tabLink } from "~/components/ui/tab-nav";
import { ago, plural } from "~/lib/format";
import { queries } from "~/lib/queries";
import { toastOutcome, trackToast } from "~/lib/toasts";
import type { ApiAdminSandbox, ApiSandboxState } from "~/lib/trex";
import { UNTITLED } from "~/lib/workspace";

import { useDeleteSandbox, useStopSandbox } from "./mutations";

const STATE: Record<ApiSandboxState, string> = {
  running: "Running",
  starting: "Starting",
  stopping: "Stopping",
  stopped: "Stopped",
  deleting: "Deleting",
  error: "Error",
  unknown: "Unknown",
};

// which sandboxes the list shows, kept in the url
const FILTERS = [
  { id: "all", label: "All" },
  { id: "leftovers", label: "Leftovers" },
  { id: "errors", label: "Errors" },
] as const;
type Filter = (typeof FILTERS)[number]["id"];

const keyOf = (sandbox: ApiAdminSandbox) => `${sandbox.workspace}/${sandbox.name}`;
const matches = (sandbox: ApiAdminSandbox, filter: Filter) => (filter === "leftovers" ? sandbox.chat === null : filter === "errors" ? sandbox.state === "error" : true);
const sandboxCount = (count: number) => plural(count, "sandbox", "sandboxes");

function SandboxMenu({ sandbox, onDelete }: { sandbox: ApiAdminSandbox; onDelete: () => void }) {
  const stop = useStopSandbox();
  // a chat that's working needs its sandbox, so it's stopped through the run instead
  const busy = sandbox.chat?.running ?? false;
  return (
    <Menu.Root>
      <Menu.Trigger aria-label={`Actions for ${sandbox.name}`} className={iconButton}>
        <LuEllipsis size={15} />
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner align="end" sideOffset={4} className="z-50">
          <Menu.Popup className={`w-52 rounded-lg p-1 ${popup}`}>
            <Menu.Item
              disabled={busy || sandbox.state !== "running"}
              onClick={() => void toastOutcome(stop.mutateAsync({ workspace: sandbox.workspace, name: sandbox.name }), { success: `Stopped ${sandbox.name}`, error: `Couldn't stop ${sandbox.name}` })}
              className={`${menuItem} data-disabled:cursor-not-allowed data-disabled:opacity-50`}
            >
              <LuSquare size={14} />
              Stop
            </Menu.Item>
            <Menu.Separator className={menuSeparator} />
            <Menu.Item disabled={busy} onClick={onDelete} className={`${dangerMenuItem} data-disabled:cursor-not-allowed data-disabled:opacity-50`}>
              <LuTrash2 size={14} />
              Delete
            </Menu.Item>
            {busy && <p className="px-2.5 pt-1 pb-1.5 text-[11px] text-muted">Its chat is running. Stop the run first.</p>}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}

const column = columnsFor<ApiAdminSandbox>();

export function SandboxList() {
  const { data: sandboxes } = useSuspenseQuery(queries.admin.sandboxes());
  const [params] = useSearchParams();
  const filter: Filter = FILTERS.find((option) => option.id === params.get("show"))?.id ?? "all";
  const [deleting, setDeleting] = useState<ApiAdminSandbox[] | null>(null);
  const remove = useDeleteSandbox();

  const leftovers = sandboxes.filter((sandbox) => sandbox.chat === null);
  const counts = { all: sandboxes.length, leftovers: leftovers.length, errors: sandboxes.filter((sandbox) => sandbox.state === "error").length };
  const running = sandboxes.filter((sandbox) => sandbox.state === "running").length;
  const shown = sandboxes.filter((sandbox) => matches(sandbox, filter));

  function confirmDelete() {
    const targets = deleting ?? [];
    setDeleting(null);
    // one at a time, so a failure says which one
    const work = (async () => {
      for (const sandbox of targets) await remove.mutateAsync({ workspace: sandbox.workspace, name: sandbox.name });
    })();
    const what = targets.length === 1 ? targets[0].name : sandboxCount(targets.length);
    void trackToast(work, { loading: `Deleting ${what}…`, success: `Deleted ${what}`, error: `Couldn't delete ${what}` }).catch(() => {});
  }

  const deleteTarget = (targets: ApiAdminSandbox[]): DeleteTarget =>
    targets.length === 1 ? { kind: "sandbox", id: keyOf(targets[0]), name: targets[0].name } : { kind: "sandboxes", id: "", name: sandboxCount(targets.length) };

  const columns = [
    column.accessor("name", {
      header: "Sandbox",
      cell: ({ row }) => (
        <span className="block min-w-0">
          <span className="block truncate font-mono text-[13px]">{row.original.name}</span>
          <span className="block truncate text-xs text-muted">{row.original.owner_email ?? row.original.workspace_name}</span>
        </span>
      ),
    }),
    column.accessor((sandbox) => sandbox.chat?.title ?? "", {
      id: "chat",
      header: "Chat",
      cell: ({ row }) =>
        row.original.chat ? (
          <span className="block min-w-0 truncate text-xs">
            {row.original.chat.title ?? UNTITLED}
            {row.original.chat.running && <span className={`ml-2 ${badge}`}>Working</span>}
          </span>
        ) : (
          <span className="text-xs text-muted" title="No chat uses it any more, so it's safe to delete">
            Leftover
          </span>
        ),
      meta: { className: "hidden md:table-cell" },
    }),
    column.accessor("state", {
      header: "State",
      cell: (info) => <span className={info.getValue() === "error" ? dangerBadge : badge}>{STATE[info.getValue()]}</span>,
    }),
    column.accessor((sandbox) => sandbox.chat?.active_at ?? 0, {
      id: "active",
      header: "Last active",
      cell: ({ row }) => <span className="text-xs text-muted">{row.original.chat ? ago(row.original.chat.active_at) : "—"}</span>,
      meta: { align: "right", className: "hidden sm:table-cell" },
    }),
    column.display({
      id: "actions",
      header: () => <span className="sr-only">Actions</span>,
      cell: ({ row }) => <SandboxMenu sandbox={row.original} onDelete={() => setDeleting([row.original])} />,
      meta: { align: "right" },
    }),
  ];

  if (sandboxes.length === 0) {
    return <EmptyState icon={LuBox} title="No sandboxes" description="Chats create a sandbox the first time the agent runs a command, and idle ones stop on their own." />;
  }
  return (
    <div>
      <p className="text-sm text-muted">
        {sandboxCount(sandboxes.length)} on the gateway, {running} running. {leftovers.length > 0 ? `${sandboxCount(leftovers.length)} no chat uses any more.` : "Every one belongs to a chat."}
      </p>
      <div className="mt-4 flex flex-wrap items-center gap-2">
        <nav aria-label="Show" className="flex items-center gap-1">
          {FILTERS.map((option) => (
            <Link
              key={option.id}
              to={option.id === "all" ? "?" : `?show=${option.id}`}
              replace
              preventScrollReset
              aria-current={filter === option.id ? "page" : undefined}
              className={`${tabLink} tabular-nums`}
            >
              {option.label} {counts[option.id]}
            </Link>
          ))}
        </nav>
        <span className="flex-1" />
        {leftovers.length > 0 && (
          <Button variant="subtleDanger" onClick={() => setDeleting(leftovers)} size="sm">
            <LuTrash2 size={14} />
            Delete {leftovers.length === 1 ? "1 leftover" : `${leftovers.length} leftovers`}
          </Button>
        )}
      </div>
      <DataTable label="Sandboxes" data={shown} columns={columns} rowId={keyOf} empty={filter === "leftovers" ? "No leftovers; every sandbox belongs to a chat." : "No sandbox is in error."} />
      <DeleteConfirmDialog
        target={deleting ? deleteTarget(deleting) : null}
        consequence={deleting?.some((sandbox) => sandbox.chat) ? " and its files; its chat gets an empty one next time" : " and its files"}
        onCancel={() => setDeleting(null)}
        onConfirm={confirmDelete}
      />
    </div>
  );
}
