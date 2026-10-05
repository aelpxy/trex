import { useMemo, useRef, useState, type KeyboardEvent, type MouseEvent } from "react";
import { ContextMenu } from "@base-ui/react/context-menu";
import { useSuspenseQuery } from "@tanstack/react-query";
import { useSearchParams } from "react-router";
import { LuFiles, LuFolderPlus, LuSquareCheck, LuUpload } from "react-icons/lu";

import { FilterInput } from "~/components/admin/filter-input";
import { useUrlFilter } from "~/components/admin/use-url-filter";
import { Button } from "~/components/ui/button";
import { DataTable, type RowSelectionState } from "~/components/ui/data-table";
import { DeleteConfirmDialog, type DeleteTarget } from "~/components/ui/delete-confirm-dialog";
import { EmptyState } from "~/components/ui/empty-state";
import { menuItem, popup } from "~/components/ui/styles";
import { queries } from "~/lib/queries";

import { Breadcrumbs } from "./breadcrumbs";
import { baseName, entriesOf, foldersOf, keyOf, parentOf, placeName, plural, viewable, type Entry } from "./entries";
import { entryColumns } from "./entry-columns";
import { EntryMenuItems, type EntryActions } from "./entry-menu";
import { MoveDialog } from "./move-dialog";
import { NameDialog } from "./name-dialog";
import { ReplaceDialog } from "./replace-dialog";
import { SelectionBar } from "./selection-bar";
import { useEntryDrag } from "./use-entry-drag";
import { useFileDrop } from "./use-file-drop";
import { useLibraryActions } from "./use-library-actions";

// the dialog open, if any
type Pending =
  | { action: "folder" }
  | { action: "rename"; entry: Entry }
  | { action: "move"; entries: Entry[] }
  | { action: "delete"; entries: Entry[] }
  | { action: "replace"; folder: string; files: File[]; names: string[] };

function deleteTarget(entries: Entry[]): DeleteTarget {
  if (entries.length > 1) return { kind: "items", id: "", name: plural(entries.length, "item") };
  const [entry] = entries;
  return { kind: entry.kind, id: entry.path, name: baseName(entry.path) };
}

export function FileManager() {
  const { data: files } = useSuspenseQuery(queries.library());
  const [params, setParams] = useSearchParams();
  const [filter, setFilter] = useUrlFilter();
  const folder = params.get("path") ?? "";
  const entries = useMemo(() => entriesOf(files, folder, filter), [files, folder, filter]);
  const [pending, setPending] = useState<Pending | null>(null);
  const input = useRef<HTMLInputElement>(null);

  // ticks belong to one folder and search, so leaving either clears them
  const view = `${folder}|${filter}`;
  const [ticked, setTicked] = useState<{ view: string; rows: RowSelectionState }>({ view, rows: {} });
  const selection = ticked.view === view ? ticked.rows : {};
  const setSelection = (rows: RowSelectionState) => setTicked({ view, rows });
  const selected = entries.filter((entry) => selection[keyOf(entry)]);
  // acting on a ticked row acts on everything ticked
  const withSelection = (entry: Entry) => (selection[keyOf(entry)] && selected.length > 1 ? selected : [entry]);
  // the rows a right-click is about; none for the space around them
  const [menuFor, setMenuFor] = useState<Entry[] | null>(null);

  const library = useLibraryActions(files, () => setSelection({}));

  // uploads that would overwrite something ask first
  function upload(target: string, picked: File[]) {
    const names = library.conflicts(target, picked);
    if (names.length > 0) setPending({ action: "replace", folder: target, files: picked, names });
    else library.upload(target, picked);
  }

  const fileDrop = useFileDrop((picked) => upload(folder, picked));
  const drag = useEntryDrag({ carried: withSelection, onMove: library.move, onUpload: upload });

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

  const actions: EntryActions = {
    onOpenFolder: (entry) => openFolder(entry.path),
    onShow: library.show,
    onRename: (entry) => setPending({ action: "rename", entry }),
    onMove: (targets) => setPending({ action: "move", entries: targets }),
    onDelete: (targets) => setPending({ action: "delete", entries: targets }),
  };

  // Delete, or Cmd+Backspace on a Mac; dialogs and menus are portals, so their keys don't count
  function onKeyDown(event: KeyboardEvent) {
    const deleting = event.key === "Delete" || (event.key === "Backspace" && event.metaKey);
    if (!deleting || selected.length === 0 || pending !== null || event.target instanceof HTMLInputElement) return;
    if (!event.currentTarget.contains(event.target as Node)) return;
    event.preventDefault();
    setPending({ action: "delete", entries: selected });
  }

  function onContextMenu(event: MouseEvent) {
    const id = (event.target as Element).closest("tr[data-row-id]")?.getAttribute("data-row-id");
    const entry = entries.find((candidate) => keyOf(candidate) === id);
    setMenuFor(entry ? withSelection(entry) : null);
  }

  // anywhere but inside a folder being moved; the open folder counts even before it holds a file
  const destinations = (targets: Entry[]) =>
    [...new Set(["", ...foldersOf(files), folder])].sort().filter((path) => !targets.some((entry) => entry.kind === "folder" && path.startsWith(entry.path)));

  const close = () => setPending(null);
  const uploadButton = (
    <Button onClick={() => input.current?.click()}>
      <LuUpload size={14} />
      Upload
    </Button>
  );

  return (
    <div {...fileDrop.props} onKeyDown={onKeyDown} className="relative mt-6">
      <input
        ref={input}
        type="file"
        multiple
        hidden
        onChange={(event) => {
          upload(folder, [...(event.target.files ?? [])]);
          event.target.value = "";
        }}
      />
      {fileDrop.over && (
        <div className="pointer-events-none absolute -inset-3 z-10 flex items-end justify-center rounded-2xl border-2 border-dashed border-accent pb-4">
          <span className="glass rounded-full border border-line px-4 py-2 text-sm font-medium shadow-lg">Drop to upload to {placeName(drag.dropTarget ?? folder)}</span>
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
            <Button variant="quiet" onClick={() => setPending({ action: "folder" })} aria-label="New folder" title="New folder" className="px-3">
              <LuFolderPlus size={15} />
              <span className="hidden sm:inline">New folder</span>
            </Button>
            {uploadButton}
          </div>
          <div className="mt-4 flex min-h-8 items-center">
            {selected.length > 0 ? (
              <SelectionBar count={selected.length} onClear={() => setSelection({})} onMove={() => actions.onMove(selected)} onDelete={() => actions.onDelete(selected)} />
            ) : filter ? (
              <p className="text-xs text-muted">{plural(entries.length, "match")} across all folders</p>
            ) : (
              <Breadcrumbs folder={folder} onOpen={openFolder} dropProps={drag.dropProps} dropTarget={drag.dropTarget} />
            )}
          </div>
          <ContextMenu.Root>
            <ContextMenu.Trigger onContextMenu={onContextMenu}>
              <DataTable
                label="Files"
                data={entries}
                columns={entryColumns(actions)}
                rowId={keyOf}
                rowLabel={(entry) => entry.name}
                selection={selection}
                onSelectionChange={setSelection}
                onRowClick={(entry) => (entry.kind === "folder" ? openFolder(entry.path) : library.show(entry, !viewable(entry.name)))}
                empty={filter ? "No file matches that search." : "This folder is empty. Upload files or drop them here; it's kept once it holds a file."}
                rowProps={(entry) => ({
                  ...drag.dragProps(entry),
                  ...(entry.kind === "folder" && drag.dropProps(entry.path)),
                  className: `${drag.dropTarget === entry.path ? "bg-accent/10 outline-2 -outline-offset-2 outline-accent" : ""} ${drag.isDragged(entry) ? "opacity-50" : ""}`,
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
          description={`Inside ${placeName(folder)}. It opens so you can upload into it.`}
          action="Create"
          onClose={close}
          onSubmit={(name) => {
            close();
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
          onClose={close}
          onSubmit={(name) => {
            close();
            library.rename(pending.entry, name);
          }}
        />
      )}
      {pending?.action === "move" && (
        <MoveDialog
          count={pending.entries.length}
          folders={destinations(pending.entries)}
          initial={parentOf(pending.entries[0].path)}
          onClose={close}
          onMove={(destination) => {
            close();
            library.move(pending.entries, destination);
          }}
        />
      )}
      {pending?.action === "replace" && (
        <ReplaceDialog
          names={pending.names}
          onCancel={close}
          onReplace={() => {
            close();
            library.upload(pending.folder, pending.files);
          }}
          onKeepBoth={() => {
            close();
            library.upload(pending.folder, pending.files, true);
          }}
        />
      )}
      <DeleteConfirmDialog
        target={pending?.action === "delete" ? deleteTarget(pending.entries) : null}
        consequence={pending?.action === "delete" && pending.entries.some((entry) => entry.kind === "folder") ? " and every file inside" : ""}
        onCancel={close}
        onConfirm={() => {
          if (pending?.action !== "delete") return;
          close();
          library.remove(pending.entries);
        }}
      />
    </div>
  );
}
