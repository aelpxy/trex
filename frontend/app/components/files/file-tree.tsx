import { useMemo, useState, type KeyboardEvent } from "react";
import { Button } from "@base-ui/react/button";
import { Collapsible } from "@base-ui/react/collapsible";
import { ContextMenu } from "@base-ui/react/context-menu";
import { LuChevronRight, LuCopy, LuDownload, LuFile, LuFilePlus, LuFolder, LuFolderOpen, LuPencil, LuRefreshCw, LuTrash2 } from "react-icons/lu";

import { openFile } from "~/components/library/entries";
import { collapsiblePanel, dangerMenuItem, focusRing, iconButton, menuItem, menuSeparator, popup } from "~/components/ui/styles";
import { sandboxFile } from "~/lib/api";
import { errorText, plural } from "~/lib/format";
import { toasts } from "~/lib/toasts";

import { useFiles, type FileStatus } from "./files-provider";
import { PathInput } from "./path-input";
import { baseName, useTreeDrag, type Dragged } from "./use-tree-drag";

type TreeNode = { name: string; path: string; children?: TreeNode[] };

const INDENT_PX = 12;
const SANDBOX_ROOT = "/sandbox/";
const row = `flex h-7 w-full cursor-pointer items-center gap-1.5 rounded-md pr-2 text-left text-xs text-muted transition-colors hover:bg-subtle hover:text-ink aria-[current=page]:bg-subtle aria-[current=page]:text-ink ${focusRing}`;
const dropHighlight = "bg-accent/10 text-ink outline-2 -outline-offset-2 outline-accent";

function buildTree(paths: string[]): TreeNode[] {
  const root: TreeNode = { name: "", path: "", children: [] };
  for (const path of paths) {
    let node = root;
    path.split("/").forEach((name, index, segments) => {
      const isFile = index === segments.length - 1;
      const nodePath = segments.slice(0, index + 1).join("/");
      let child = node.children!.find((candidate) => candidate.name === name && !candidate.children === isFile);
      if (!child) {
        child = isFile ? { name, path: nodePath } : { name, path: nodePath, children: [] };
        node.children!.push(child);
      }
      node = child;
    });
  }
  const sort = (nodes: TreeNode[]): TreeNode[] =>
    nodes
      .map((node) => (node.children ? { ...node, children: sort(node.children) } : node))
      .sort((a, b) => Number(!a.children) - Number(!b.children) || a.name.localeCompare(b.name));
  return sort(root.children!);
}

const STATUS_LABEL: Record<NonNullable<FileStatus>, string> = { new: "New", edited: "Edited" };

// what rows can ask the tree to do
type TreeActions = {
  startRename: (dragged: Dragged) => void;
  finishRename: (dragged: Dragged, to: string) => void;
  cancelRename: () => void;
  newFileIn: (folder: string) => void;
  remove: (dragged: Dragged) => void;
  download: (path: string) => void;
  drag: ReturnType<typeof useTreeDrag>;
  renaming: Dragged | null;
};

const copyPath = (path: string) =>
  navigator.clipboard
    .writeText(`${SANDBOX_ROOT}${path}`)
    .then(() => toasts.add({ title: "Path copied", description: `${SANDBOX_ROOT}${path}`, type: "success" }))
    .catch((cause) => toasts.add({ title: "Couldn't copy the path", description: errorText(cause), type: "error" }));

// F2 renames and Delete deletes the focused row, as in desktop file managers
const rowKeys = (dragged: Dragged, actions: TreeActions) => (event: KeyboardEvent) => {
  if (event.key === "F2") {
    event.preventDefault();
    actions.startRename(dragged);
  } else if (event.key === "Delete") {
    event.preventDefault();
    actions.remove(dragged);
  }
};

function FileRow({ node, depth, actions }: { node: TreeNode; depth: number; actions: TreeActions }) {
  const { openPath, open, read } = useFiles();
  const { status } = read(node.path);
  const dragged = { path: node.path, folder: false };

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger className="block">
        <Button
          {...actions.drag.dragProps(dragged)}
          onClick={() => open(node.path)}
          onKeyDown={rowKeys(dragged, actions)}
          aria-current={openPath === node.path ? "page" : undefined}
          title={node.path}
          className={`${row} ${actions.drag.dragging?.path === node.path ? "opacity-50" : ""}`}
          style={{ paddingLeft: 8 + depth * INDENT_PX + 16 }}
        >
          <LuFile size={13} className="shrink-0" />
          <span className="min-w-0 flex-1 truncate">{node.name}</span>
          {status && (
            <span title={STATUS_LABEL[status]} className={`size-1.5 shrink-0 rounded-full ${status === "new" ? "bg-ink" : "bg-muted"}`}>
              <span className="sr-only">{STATUS_LABEL[status]}</span>
            </span>
          )}
        </Button>
      </ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Positioner className="z-50">
          <ContextMenu.Popup className={`w-48 rounded-lg p-1 ${popup}`}>
            <ContextMenu.Item onClick={() => open(node.path)} className={menuItem}>
              <LuFile size={14} />
              Open
            </ContextMenu.Item>
            <ContextMenu.Item onClick={() => actions.startRename(dragged)} className={menuItem}>
              <LuPencil size={14} />
              Rename
            </ContextMenu.Item>
            <ContextMenu.Item onClick={() => void copyPath(node.path)} className={menuItem}>
              <LuCopy size={14} />
              Copy path
            </ContextMenu.Item>
            <ContextMenu.Item onClick={() => actions.download(node.path)} className={menuItem}>
              <LuDownload size={14} />
              Download
            </ContextMenu.Item>
            <ContextMenu.Separator className={menuSeparator} />
            <ContextMenu.Item onClick={() => actions.remove(dragged)} className={dangerMenuItem}>
              <LuTrash2 size={14} />
              Delete
            </ContextMenu.Item>
          </ContextMenu.Popup>
        </ContextMenu.Positioner>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

function FolderRow({ node, depth, actions }: { node: TreeNode; depth: number; actions: TreeActions }) {
  const dragged = { path: node.path, folder: true };
  return (
    <Collapsible.Root defaultOpen>
      <ContextMenu.Root>
        <ContextMenu.Trigger className="block">
          <Collapsible.Trigger
            {...actions.drag.dragProps(dragged)}
            {...actions.drag.dropProps(node.path)}
            onKeyDown={rowKeys(dragged, actions)}
            className={`group ${row} ${actions.drag.dropTarget === node.path ? dropHighlight : ""} ${actions.drag.dragging?.path === node.path ? "opacity-50" : ""}`}
            style={{ paddingLeft: 8 + depth * INDENT_PX }}
          >
            <LuChevronRight size={12} className="shrink-0 transition-transform duration-150 group-data-panel-open:rotate-90" />
            <LuFolder size={13} className="shrink-0 group-data-panel-open:hidden" />
            <LuFolderOpen size={13} className="hidden shrink-0 group-data-panel-open:block" />
            <span className="truncate">{node.name}</span>
          </Collapsible.Trigger>
        </ContextMenu.Trigger>
        <ContextMenu.Portal>
          <ContextMenu.Positioner className="z-50">
            <ContextMenu.Popup className={`w-48 rounded-lg p-1 ${popup}`}>
              <ContextMenu.Item onClick={() => actions.newFileIn(node.path)} className={menuItem}>
                <LuFilePlus size={14} />
                New file here
              </ContextMenu.Item>
              <ContextMenu.Item onClick={() => actions.startRename(dragged)} className={menuItem}>
                <LuPencil size={14} />
                Rename
              </ContextMenu.Item>
              <ContextMenu.Item onClick={() => void copyPath(node.path)} className={menuItem}>
                <LuCopy size={14} />
                Copy path
              </ContextMenu.Item>
              <ContextMenu.Separator className={menuSeparator} />
              <ContextMenu.Item onClick={() => actions.remove(dragged)} className={dangerMenuItem}>
                <LuTrash2 size={14} />
                Delete folder
              </ContextMenu.Item>
            </ContextMenu.Popup>
          </ContextMenu.Positioner>
        </ContextMenu.Portal>
      </ContextMenu.Root>
      <Collapsible.Panel className={collapsiblePanel}>
        <TreeItems nodes={node.children ?? []} depth={depth + 1} actions={actions} />
      </Collapsible.Panel>
    </Collapsible.Root>
  );
}

function RenameRow({ node, actions }: { node: TreeNode; actions: TreeActions }) {
  const { paths } = useFiles();
  const renaming = actions.renaming!;
  const taken = (path: string) => path !== node.path && (paths.includes(path) || paths.some((existing) => existing.startsWith(`${path}/`)));
  return (
    <PathInput
      label={`Rename ${node.path}`}
      initial={node.path}
      validate={(path) => (taken(path) ? "Something already has that path." : renaming.folder && path.startsWith(`${node.path}/`) ? "A folder can't go inside itself." : null)}
      onSubmit={(path) => actions.finishRename(renaming, path)}
      onCancel={actions.cancelRename}
    />
  );
}

function TreeItems({ nodes, depth, actions }: { nodes: TreeNode[]; depth: number; actions: TreeActions }) {
  return (
    <ul className="space-y-px">
      {nodes.map((node) => (
        <li key={node.path}>
          {actions.renaming?.path === node.path ? (
            <RenameRow node={node} actions={actions} />
          ) : node.children ? (
            <FolderRow node={node} depth={depth} actions={actions} />
          ) : (
            <FileRow node={node} depth={depth} actions={actions} />
          )}
        </li>
      ))}
    </ul>
  );
}

export function FileTree() {
  const { paths, create, refresh, loading, error, canBrowse, rename, requestRemove, upload, sessionId } = useFiles();
  // the new file's path so far, e.g. "src/" from a folder's menu
  const [creating, setCreating] = useState<string | null>(null);
  const [renaming, setRenaming] = useState<Dragged | null>(null);
  const tree = useMemo(() => buildTree(paths), [paths]);

  // moves only start once none would land on an existing path, so a folder never ends up half moved
  function relocate(moves: { from: string; to: string }[], done: string) {
    const clash = moves.find((move) => paths.includes(move.to));
    if (clash) {
      toasts.add({ title: `${clash.to} already exists`, description: "Rename one of them first.", type: "error" });
      return;
    }
    moves.forEach((move) => rename(move.from, move.to));
    toasts.add({ title: done, type: "success" });
  }

  // a file's path, or every file under a folder, moved from `from` to `to`
  const movesFor = (dragged: Dragged, to: string) =>
    dragged.folder ? paths.filter((path) => path.startsWith(`${dragged.path}/`)).map((path) => ({ from: path, to: `${to}${path.slice(dragged.path.length)}` })) : [{ from: dragged.path, to }];

  const drag = useTreeDrag({
    onMove: (dragged, folder) => {
      const to = folder ? `${folder}/${baseName(dragged.path)}` : baseName(dragged.path);
      relocate(movesFor(dragged, to), `Moved ${baseName(dragged.path)} to ${folder || "the top level"}`);
    },
    onUpload: (folder, files) => {
      if (files.length === 0) return;
      const prefix = folder ? `${folder}/` : "";
      void Promise.all(files.map((file) => upload(`${prefix}${file.name}`, file))).then((results) => {
        const uploaded = results.filter(Boolean).length;
        if (uploaded) toasts.add({ title: `Uploaded ${plural(uploaded, "file")} to ${folder || "the top level"}`, type: "success" });
      });
    },
  });

  const actions: TreeActions = {
    startRename: setRenaming,
    finishRename: (target, to) => {
      setRenaming(null);
      if (to !== target.path) relocate(movesFor(target, to), `Renamed to ${baseName(to)}`);
    },
    cancelRename: () => setRenaming(null),
    newFileIn: (folder) => setCreating(`${folder}/`),
    remove: (target) => requestRemove(target.path, target.folder),
    download: (path) => {
      if (!sessionId) return;
      openFile(() => sandboxFile(sessionId, path), path, true).catch((cause) => toasts.add({ title: `Couldn't download ${baseName(path)}`, description: errorText(cause), type: "error" }));
    },
    drag,
    renaming,
  };

  return (
    <div className="flex h-full flex-col">
      <div className="flex h-9 shrink-0 items-center justify-between pr-1 pl-3">
        <span className="text-xs font-medium text-muted">Files</span>
        <div className="flex items-center">
          {canBrowse && (
            <Button onClick={refresh} disabled={loading} aria-label="Refresh files" title="Refresh" className={`${iconButton} size-7`}>
              <LuRefreshCw size={13} className={loading ? "animate-spin" : undefined} />
            </Button>
          )}
          <Button onClick={() => setCreating("")} aria-label="New file" title="New file" className={`${iconButton} size-7`}>
            <LuFilePlus size={14} />
          </Button>
        </div>
      </div>
      {error && (
        <p role="alert" className="px-3 pb-1.5 text-xs text-danger">
          {error}
        </p>
      )}
      <ContextMenu.Root>
        {/* the empty space is the top level: drop there to move something out of its folders */}
        <ContextMenu.Trigger
          render={<nav aria-label="Files" />}
          {...drag.dropProps("")}
          className={`min-h-0 flex-1 overflow-y-auto rounded-md px-1 pb-2 ${drag.dropTarget === "" ? dropHighlight : ""}`}
        >
          {creating !== null && (
            <PathInput
              label="New file path"
              initial={creating}
              validate={(path) => (paths.includes(path) ? "A file with that path already exists." : null)}
              onSubmit={(path) => {
                create(path);
                setCreating(null);
              }}
              onCancel={() => setCreating(null)}
            />
          )}
          {tree.length === 0 && creating === null ? (
            <p className="px-2 py-1.5 text-xs text-muted">{loading ? "Loading files…" : "No files yet. Drop files here to upload them."}</p>
          ) : (
            <TreeItems nodes={tree} depth={0} actions={actions} />
          )}
        </ContextMenu.Trigger>
        <ContextMenu.Portal>
          <ContextMenu.Positioner className="z-50">
            <ContextMenu.Popup className={`w-48 rounded-lg p-1 ${popup}`}>
              <ContextMenu.Item onClick={() => setCreating("")} className={menuItem}>
                <LuFilePlus size={14} />
                New file
              </ContextMenu.Item>
              {canBrowse && (
                <ContextMenu.Item onClick={refresh} className={menuItem}>
                  <LuRefreshCw size={14} />
                  Refresh
                </ContextMenu.Item>
              )}
            </ContextMenu.Popup>
          </ContextMenu.Positioner>
        </ContextMenu.Portal>
      </ContextMenu.Root>
    </div>
  );
}
