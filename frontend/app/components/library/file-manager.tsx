import { useRef, useState, type DragEvent, type HTMLAttributes, type KeyboardEvent, type MouseEvent } from "react";
import { ContextMenu } from "@base-ui/react/context-menu";
import { Menu } from "@base-ui/react/menu";
import { useSuspenseQuery } from "@tanstack/react-query";
import { useSearchParams } from "react-router";
import { LuDownload, LuEllipsis, LuExternalLink, LuFiles, LuFolderInput, LuFolderOpen, LuFolderPlus, LuPencil, LuSquareCheck, LuTrash2, LuUpload } from "react-icons/lu";

import { ActionStatus } from "~/components/admin/action-status";
import { FilterInput } from "~/components/admin/filter-input";
import { date } from "~/components/admin/format";
import { useUrlFilter } from "~/components/admin/use-url-filter";
import { Button } from "~/components/ui/button";
import { columnsFor, DataTable, type RowSelectionState } from "~/components/ui/data-table";
import { DeleteConfirmDialog, type DeleteTarget } from "~/components/ui/delete-confirm-dialog";
import { EmptyState } from "~/components/ui/empty-state";
import { dangerMenuItem, iconButton, menuItem, menuSeparator, popup } from "~/components/ui/styles";
import { download } from "~/lib/api";
import { queries } from "~/lib/queries";

import { Breadcrumbs } from "./breadcrumbs";
import { baseName, entriesOf, entryIcon, filesOf, foldersOf, formatSize, openFile, viewable, type Entry } from "./entries";
import { MoveDialog } from "./move-dialog";
import { useDeleteFiles, useMoveFiles, useUploadFiles, type Move } from "./mutations";
import { NameDialog } from "./name-dialog";

const keyOf = (entry: Entry) => `${entry.kind}:${entry.path}`;
const parentOf = (path: string) => path.slice(0, path.replace(/\/$/, "").lastIndexOf("/") + 1);
const plural = (count: number, noun: string) => `${count.toLocaleString()} ${noun}${count === 1 ? "" : "s"}`;

// where each file goes when `entry` moves to sit under `folder` as `name`
function movesOf(entry: Entry, paths: string[], folder: string, name = baseName(entry.path)): Move[] {
  if (entry.kind === "file") return [{ from: entry.path, to: `${folder}${name}` }];
  return paths.map((path) => ({ from: path, to: `${folder}${name}/${path.slice(entry.path.length)}` }));
}

// what the user is doing to some entries, each with its own dialog
type Pending = { action: "rename"; entry: Entry } | { action: "move"; entries: Entry[] } | { action: "delete"; entries: Entry[] } | { action: "folder" };

// marks a drag of library rows, so folders tell them apart from files off the desktop
const ENTRIES_TYPE = "application/x-library-entries";

type EntryActions = {
  onOpenFolder: (entry: Entry) => void;
  onShow: (entry: Entry, download: boolean) => void;
  onRename: (entry: Entry) => void;
  onMove: (entries: Entry[]) => void;
  onDelete: (entries: Entry[]) => void;
};

// what can be done to one entry, or to several at once; shared by the row menu and right-click
function EntryMenuItems({ targets, onOpenFolder, onShow, onRename, onMove, onDelete }: EntryActions & { targets: Entry[] }) {
  const [entry] = targets;
  if (targets.length > 1) {
    return (
      <>
        <Menu.Item onClick={() => onMove(targets)} className={menuItem}>
          <LuFolderInput size={14} />
          Move {plural(targets.length, "item")}…
        </Menu.Item>
        <Menu.Separator className={menuSeparator} />
        <Menu.Item onClick={() => onDelete(targets)} className={dangerMenuItem}>
          <LuTrash2 size={14} />
          Delete {plural(targets.length, "item")}
        </Menu.Item>
      </>
    );
  }
  return (
    <>
      {entry.kind === "folder" ? (
        <Menu.Item onClick={() => onOpenFolder(entry)} className={menuItem}>
          <LuFolderOpen size={14} />
          Open
        </Menu.Item>
      ) : (
        <>
          {viewable(entry.name) && (
            <Menu.Item onClick={() => onShow(entry, false)} className={menuItem}>
              <LuExternalLink size={14} />
              Open in a new tab
            </Menu.Item>
          )}
          <Menu.Item onClick={() => onShow(entry, true)} className={menuItem}>
            <LuDownload size={14} />
            Download
          </Menu.Item>
        </>
      )}
      <Menu.Separator className={menuSeparator} />
      <Menu.Item onClick={() => onRename(entry)} className={menuItem}>
        <LuPencil size={14} />
        Rename
      </Menu.Item>
      <Menu.Item onClick={() => onMove(targets)} className={menuItem}>
        <LuFolderInput size={14} />
        Move to…
      </Menu.Item>
      <Menu.Separator className={menuSeparator} />
      <Menu.Item onClick={() => onDelete(targets)} className={dangerMenuItem}>
        <LuTrash2 size={14} />
        Delete
      </Menu.Item>
    </>
  );
}

// files from the desktop dropped anywhere on the manager; `over` shows the drop hint
function useFileDrop(onFiles: (files: File[]) => void) {
  const [over, setOver] = useState(false);
  // dragenter and dragleave fire for every child crossed, so count them
  const depth = useRef(0);
  const accepts = (event: DragEvent) => event.dataTransfer.types.includes("Files");
  return {
    over,
    props: {
      onDragEnter: (event: DragEvent) => {
        if (!accepts(event)) return;
        event.preventDefault();
        depth.current += 1;
        setOver(true);
      },
      onDragOver: (event: DragEvent) => {
        if (!accepts(event)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = "copy";
      },
      onDragLeave: (event: DragEvent) => {
        if (!accepts(event)) return;
        depth.current = Math.max(0, depth.current - 1);
        if (depth.current === 0) setOver(false);
      },
      onDrop: (event: DragEvent) => {
        if (!accepts(event)) return;
        depth.current = 0;
        setOver(false);
        // a folder or breadcrumb took it already
        if (event.defaultPrevented) return;
        event.preventDefault();
        onFiles([...event.dataTransfer.files]);
      },
    },
  };
}

export function FileManager() {
  const { data: files } = useSuspenseQuery(queries.library());
  const [params, setParams] = useSearchParams();
  const [filter, setFilter] = useUrlFilter();
  const folder = params.get("path") ?? "";
  const entries = entriesOf(files, folder, filter);
  const [pending, setPending] = useState<Pending | null>(null);
  const [error, setError] = useState<unknown>(null);
  const input = useRef<HTMLInputElement>(null);
  const uploads = useUploadFiles();
  const moves = useMoveFiles();
  const deletes = useDeleteFiles();
  const busy = uploads.isPending || moves.isPending || deletes.isPending;

  // ticks belong to one folder and search, so leaving either clears them
  const view = `${folder}|${filter}`;
  const [ticked, setTicked] = useState<{ view: string; rows: RowSelectionState }>({ view, rows: {} });
  const selection = ticked.view === view ? ticked.rows : {};
  const setSelection = (rows: RowSelectionState) => setTicked({ view, rows });
  const selected = entries.filter((entry) => selection[keyOf(entry)]);
  // the rows a right-click is about; none for the empty space around them
  const [menuFor, setMenuFor] = useState<Entry[] | null>(null);
  // the rows being dragged, and the folder they'd land in
  const [dragging, setDragging] = useState<Entry[] | null>(null);
  const [dropTarget, setDropTarget] = useState<string | null>(null);

  const openFolder = (path: string) =>
    setParams(
      (current) => {
        const next = new URLSearchParams(current);
        if (path) next.set("path", path);
        else next.delete("path");
        next.delete("q");
        return next;
      },
      { preventScrollReset: true },
    );

  function run<T>(mutation: { mutate: (items: T[], options: { onSuccess: () => void }) => void }, items: T[]) {
    setError(null);
    setPending(null);
    mutation.mutate(items, { onSuccess: () => setSelection({}) });
  }

  const uploadTo = (target: string, picked: File[]) => picked.length > 0 && run(uploads, picked.map((file) => ({ path: `${target}${file.name}`, file })));
  const uploadHere = (picked: File[]) => uploadTo(folder, picked);
  const drop = useFileDrop(uploadHere);

  function show(entry: Entry, asDownload: boolean) {
    setError(null);
    openFile(() => download(entry.path), entry.path, asDownload).catch(setError);
  }

  function rename(entry: Entry, name: string) {
    run(moves, movesOf(entry, filesOf(entry, files), parentOf(entry.path), name));
  }

  function moveTo(targets: Entry[], destination: string) {
    run(
      moves,
      targets.flatMap((entry) => movesOf(entry, filesOf(entry, files), destination)),
    );
  }

  // acting on a selected row acts on the whole selection
  const withSelection = (entry: Entry) => (selection[keyOf(entry)] && selected.length > 1 ? selected : [entry]);

  // dropping into a folder can't put a folder inside itself, and has to move at least one thing
  const canDrop = (path: string, targets: Entry[]) =>
    !targets.some((entry) => entry.kind === "folder" && path.startsWith(entry.path)) && targets.some((entry) => parentOf(entry.path) !== path);

  const dragProps = (entry: Entry): HTMLAttributes<HTMLElement> => ({
    draggable: true,
    onDragStart: (event) => {
      const targets = withSelection(entry);
      setDragging(targets);
      event.dataTransfer.setData(ENTRIES_TYPE, targets.map(keyOf).join("\n"));
      event.dataTransfer.effectAllowed = "move";
    },
    onDragEnd: () => {
      setDragging(null);
      setDropTarget(null);
    },
  });

  // a folder rows and files from the desktop can be dropped onto
  const dropProps = (path: string): HTMLAttributes<HTMLElement> => {
    const accepts = (event: DragEvent) => (event.dataTransfer.types.includes(ENTRIES_TYPE) ? dragging !== null && canDrop(path, dragging) : event.dataTransfer.types.includes("Files"));
    return {
      onDragOver: (event) => {
        if (!accepts(event)) return;
        event.preventDefault();
        event.dataTransfer.dropEffect = dragging ? "move" : "copy";
        setDropTarget(path);
      },
      onDragLeave: (event) => {
        if (event.currentTarget.contains(event.relatedTarget as Node | null)) return;
        setDropTarget((current) => (current === path ? null : current));
      },
      onDrop: (event) => {
        if (!accepts(event)) return;
        event.preventDefault();
        setDropTarget(null);
        if (dragging) moveTo(dragging, path);
        else uploadTo(path, [...event.dataTransfer.files]);
        setDragging(null);
      },
    };
  };

  const actions: EntryActions = {
    onOpenFolder: (entry) => openFolder(entry.path),
    onShow: show,
    onRename: (entry) => setPending({ action: "rename", entry }),
    onMove: (targets) => setPending({ action: "move", entries: targets }),
    onDelete: (targets) => setPending({ action: "delete", entries: targets }),
  };

  function onKeyDown(event: KeyboardEvent) {
    if (event.key !== "Delete" || selected.length === 0 || event.target instanceof HTMLInputElement) return;
    event.preventDefault();
    setPending({ action: "delete", entries: selected });
  }

  function onContextMenu(event: MouseEvent) {
    const id = (event.target as Element).closest("tr[data-row-id]")?.getAttribute("data-row-id");
    const entry = entries.find((candidate) => keyOf(candidate) === id);
    setMenuFor(entry ? withSelection(entry) : null);
  }

  // a folder can't go inside itself
  const destinations = (targets: Entry[]) => ["", ...foldersOf(files), ...(folder && !foldersOf(files).includes(folder) ? [folder] : [])].filter((path) => !targets.some((entry) => entry.kind === "folder" && path.startsWith(entry.path)));

  function deleteTarget(targets: Entry[]): DeleteTarget {
    if (targets.length > 1) return { kind: "items", id: "", name: `${plural(targets.length, "item")}` };
    const [entry] = targets;
    return { kind: entry.kind, id: entry.path, name: baseName(entry.path) };
  }

  const column = columnsFor<Entry>();
  const columns = [
    column.accessor("name", {
      header: "Name",
      cell: ({ row }) => {
        const Icon = entryIcon(row.original);
        return (
          <span className="flex min-w-0 items-center gap-3">
            <Icon size={16} className="shrink-0 text-muted" />
            <span className="min-w-0">
              <span className={`block truncate ${row.original.kind === "folder" ? "font-medium" : ""}`} title={row.original.path}>
                {row.original.name}
              </span>
              {row.original.kind === "folder" && <span className="block text-xs text-muted">{plural(row.original.files, "file")}</span>}
            </span>
          </span>
        );
      },
    }),
    column.accessor("size", { header: "Size", cell: (info) => <span className="text-xs text-muted">{formatSize(info.getValue())}</span>, meta: { align: "right", className: "hidden sm:table-cell" } }),
    column.accessor("modified", { header: "Modified", cell: (info) => <span className="text-xs text-muted">{date(info.getValue())}</span>, meta: { align: "right", className: "hidden md:table-cell" } }),
    column.display({
      id: "actions",
      header: () => <span className="sr-only">Actions</span>,
      cell: ({ row }) => {
        const entry = row.original;
        return (
          <span className="flex justify-end" onClick={(event) => event.stopPropagation()}>
            <Menu.Root>
              <Menu.Trigger aria-label={`Actions for ${entry.name}`} className={iconButton}>
                <LuEllipsis size={15} />
              </Menu.Trigger>
              <Menu.Portal>
                <Menu.Positioner align="end" sideOffset={4} className="z-50">
                  <Menu.Popup className={`w-48 rounded-lg p-1 ${popup}`}>
                    <EntryMenuItems targets={[entry]} {...actions} />
                  </Menu.Popup>
                </Menu.Positioner>
              </Menu.Portal>
            </Menu.Root>
          </span>
        );
      },
      meta: { align: "right" },
    }),
  ];

  const uploadButton = (
    <Button onClick={() => input.current?.click()} disabled={busy}>
      <LuUpload size={14} />
      {uploads.isPending ? "Uploading…" : "Upload"}
    </Button>
  );

  return (
    <div {...drop.props} onKeyDown={onKeyDown} className="relative mt-6">
      <input
        ref={input}
        type="file"
        multiple
        hidden
        onChange={(event) => {
          uploadHere([...(event.target.files ?? [])]);
          event.target.value = "";
        }}
      />
      {drop.over && (
        <div className="pointer-events-none absolute -inset-3 z-10 flex items-end justify-center rounded-2xl border-2 border-dashed border-accent pb-4 text-sm font-medium">
          <span className="glass rounded-full border border-line px-4 py-2 shadow-lg">
          Drop to upload to {(dropTarget ?? folder) ? baseName(dropTarget ?? folder) : "your library"}</span>
        </div>
      )}
      {files.length === 0 && !folder ? (
        <EmptyState icon={LuFiles} title="No files yet" description="Upload files or drop them here to use them across chats. Files the agent saves show up here too." action={uploadButton} />
      ) : (
        <>
          <div className="flex items-center gap-2">
            <div className="min-w-0 flex-1">
              <FilterInput value={filter} onChange={setFilter} label="Search every file" />
            </div>
            <Button variant="quiet" onClick={() => setPending({ action: "folder" })} disabled={busy} aria-label="New folder" title="New folder" className="px-3">
              <LuFolderPlus size={15} />
              <span className="hidden sm:inline">New folder</span>
            </Button>
            {uploadButton}
          </div>
          <div className="mt-4 flex min-h-8 flex-wrap items-center gap-x-3 gap-y-2">
            {selected.length > 0 ? (
              <>
                <p className="text-xs font-medium tabular-nums">{plural(selected.length, "item")} selected</p>
                <Button variant="quiet" onClick={() => setSelection({})} disabled={busy} className="h-8 px-3 text-xs">
                  Clear
                </Button>
                <span className="flex-1" />
                <Button variant="quiet" onClick={() => setPending({ action: "move", entries: selected })} disabled={busy} className="h-8 px-3 text-xs">
                  <LuFolderInput size={14} />
                  Move
                </Button>
                <Button variant="subtleDanger" onClick={() => setPending({ action: "delete", entries: selected })} disabled={busy} className="h-8 px-3 text-xs">
                  <LuTrash2 size={14} />
                  Delete
                </Button>
              </>
            ) : filter ? (
              <p className="text-xs text-muted">{plural(entries.length, "match")} across all folders</p>
            ) : (
              <Breadcrumbs folder={folder} onOpen={openFolder} dropProps={dropProps} dropTarget={dropTarget} />
            )}
          </div>
          <ActionStatus error={error ?? uploads.error ?? moves.error ?? deletes.error} success={moves.isPending ? "Moving…" : deletes.isPending ? "Deleting…" : null} />
          <ContextMenu.Root>
            <ContextMenu.Trigger onContextMenu={onContextMenu}>
              <DataTable
                label="Files"
                data={entries}
                columns={columns}
                rowId={keyOf}
                selection={selection}
                onSelectionChange={setSelection}
                onRowClick={(entry) => (entry.kind === "folder" ? openFolder(entry.path) : show(entry, !viewable(entry.name)))}
                empty={filter ? "No file matches that search." : "This folder is empty. Upload files or drop them here; it's kept once it holds a file."}
                rowProps={(entry) => ({
                  ...dragProps(entry),
                  ...(entry.kind === "folder" && dropProps(entry.path)),
                  className: `${dropTarget === entry.path ? "bg-accent/10 outline-2 -outline-offset-2 outline-accent" : ""} ${dragging?.some((dragged) => keyOf(dragged) === keyOf(entry)) ? "opacity-50" : ""}`,
                })}
              />
            </ContextMenu.Trigger>
            <ContextMenu.Portal>
              <ContextMenu.Positioner className="z-50">
                <ContextMenu.Popup className={`w-52 rounded-lg p-1 ${popup}`}>
                  {menuFor ? (
                    <EntryMenuItems targets={menuFor} {...actions} />
                  ) : (
                    <>
                      <ContextMenu.Item onClick={() => setPending({ action: "folder" })} className={menuItem}>
                        <LuFolderPlus size={14} />
                        New folder
                      </ContextMenu.Item>
                      <ContextMenu.Item onClick={() => input.current?.click()} className={menuItem}>
                        <LuUpload size={14} />
                        Upload files
                      </ContextMenu.Item>
                      {entries.length > 0 && (
                        <ContextMenu.Item onClick={() => setSelection(Object.fromEntries(entries.map((entry) => [keyOf(entry), true])))} className={menuItem}>
                          <LuSquareCheck size={14} />
                          Select all
                        </ContextMenu.Item>
                      )}
                    </>
                  )}
                </ContextMenu.Popup>
              </ContextMenu.Positioner>
            </ContextMenu.Portal>
          </ContextMenu.Root>
        </>
      )}
      {pending?.action === "folder" && (
        <NameDialog
          title="New folder"
          description={`Inside ${folder ? baseName(folder) : "your library"}. It opens so you can upload into it.`}
          action="Create"
          onClose={() => setPending(null)}
          onSubmit={(name) => {
            setPending(null);
            openFolder(`${folder}${name}/`);
          }}
        />
      )}
      {pending?.action === "rename" && (
        <NameDialog
          title={`Rename ${pending.entry.kind}`}
          description={pending.entry.kind === "folder" ? `Renames it for all ${plural(pending.entry.files, "file")} inside.` : "Nothing is replaced if the name is taken."}
          initial={baseName(pending.entry.path)}
          action="Rename"
          onClose={() => setPending(null)}
          onSubmit={(name) => rename(pending.entry, name)}
        />
      )}
      {pending?.action === "move" && (
        <MoveDialog count={pending.entries.length} folders={destinations(pending.entries)} initial={parentOf(pending.entries[0].path)} onClose={() => setPending(null)} onMove={(destination) => moveTo(pending.entries, destination)} />
      )}
      <DeleteConfirmDialog
        target={pending?.action === "delete" ? deleteTarget(pending.entries) : null}
        consequence={pending?.action === "delete" && pending.entries.some((entry) => entry.kind === "folder") ? " and every file inside" : ""}
        onCancel={() => setPending(null)}
        onConfirm={() => pending?.action === "delete" && run(deletes, pending.entries.flatMap((entry) => filesOf(entry, files)))}
      />
    </div>
  );
}
