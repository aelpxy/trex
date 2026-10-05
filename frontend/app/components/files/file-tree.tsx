import { useMemo, useState } from "react";
import { Button } from "@base-ui/react/button";
import { Collapsible } from "@base-ui/react/collapsible";
import { ContextMenu } from "@base-ui/react/context-menu";
import { LuChevronRight, LuFile, LuFilePlus, LuFolder, LuFolderOpen, LuPencil, LuRefreshCw, LuTrash2 } from "react-icons/lu";

import { collapsiblePanel, dangerMenuItem, focusRing, iconButton, menuItem, popup } from "~/components/ui/styles";

import { useFiles, type FileStatus } from "./files-provider";
import { PathInput } from "./path-input";

type TreeNode = { name: string; path: string; children?: TreeNode[] };

const INDENT_PX = 12;
const row = `flex h-7 w-full cursor-pointer items-center gap-1.5 rounded-md pr-2 text-left text-xs text-muted transition-colors hover:bg-subtle hover:text-ink aria-[current=page]:bg-subtle aria-[current=page]:text-ink ${focusRing}`;

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

function FileRow({ node, depth, renaming, onRename }: { node: TreeNode; depth: number; renaming: boolean; onRename: (path: string | null) => void }) {
  const { openPath, open, read, rename, requestRemove, paths } = useFiles();
  const { status } = read(node.path);

  if (renaming) {
    return (
      <PathInput
        label={`Rename ${node.path}`}
        initial={node.path}
        validate={(path) => (path !== node.path && paths.includes(path) ? "A file with that path already exists." : null)}
        onSubmit={(path) => {
          if (path !== node.path) rename(node.path, path);
          onRename(null);
        }}
        onCancel={() => onRename(null)}
      />
    );
  }

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger className="block">
        <Button onClick={() => open(node.path)} aria-current={openPath === node.path ? "page" : undefined} title={node.path} className={row} style={{ paddingLeft: 8 + depth * INDENT_PX + 16 }}>
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
          <ContextMenu.Popup className={`w-44 rounded-lg p-1 ${popup}`}>
            <ContextMenu.Item onClick={() => onRename(node.path)} className={menuItem}>
              <LuPencil size={14} />
              Rename
            </ContextMenu.Item>
            <ContextMenu.Item onClick={() => requestRemove(node.path)} className={dangerMenuItem}>
              <LuTrash2 size={14} />
              Delete file
            </ContextMenu.Item>
          </ContextMenu.Popup>
        </ContextMenu.Positioner>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}

function TreeItems({ nodes, depth, renamingPath, onRename }: { nodes: TreeNode[]; depth: number; renamingPath: string | null; onRename: (path: string | null) => void }) {
  return (
    <ul className="space-y-px">
      {nodes.map((node) => (
        <li key={node.path}>
          {node.children ? (
            <Collapsible.Root defaultOpen>
              <Collapsible.Trigger className={`group ${row}`} style={{ paddingLeft: 8 + depth * INDENT_PX }}>
                <LuChevronRight size={12} className="shrink-0 transition-transform duration-150 group-data-panel-open:rotate-90" />
                <LuFolder size={13} className="shrink-0 group-data-panel-open:hidden" />
                <LuFolderOpen size={13} className="hidden shrink-0 group-data-panel-open:block" />
                <span className="truncate">{node.name}</span>
              </Collapsible.Trigger>
              <Collapsible.Panel className={collapsiblePanel}>
                <TreeItems nodes={node.children} depth={depth + 1} renamingPath={renamingPath} onRename={onRename} />
              </Collapsible.Panel>
            </Collapsible.Root>
          ) : (
            <FileRow node={node} depth={depth} renaming={renamingPath === node.path} onRename={onRename} />
          )}
        </li>
      ))}
    </ul>
  );
}

export function FileTree() {
  const { paths, create, refresh, loading, error, canBrowse } = useFiles();
  const [creating, setCreating] = useState(false);
  const [renamingPath, setRenamingPath] = useState<string | null>(null);
  const tree = useMemo(() => buildTree(paths), [paths]);

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
          <Button onClick={() => setCreating(true)} aria-label="New file" title="New file" className={`${iconButton} size-7`}>
            <LuFilePlus size={14} />
          </Button>
        </div>
      </div>
      {error && (
        <p role="alert" className="px-3 pb-1.5 text-xs text-danger">
          {error}
        </p>
      )}
      <nav aria-label="Files" className="min-h-0 flex-1 overflow-y-auto px-1 pb-2">
        {creating && (
          <PathInput
            label="New file path"
            validate={(path) => (paths.includes(path) ? "A file with that path already exists." : null)}
            onSubmit={(path) => {
              create(path);
              setCreating(false);
            }}
            onCancel={() => setCreating(false)}
          />
        )}
        {tree.length === 0 && !creating ? (
          <p className="px-2 py-1.5 text-xs text-muted">{loading ? "Loading files…" : "No files yet"}</p>
        ) : (
          <TreeItems nodes={tree} depth={0} renamingPath={renamingPath} onRename={setRenamingPath} />
        )}
      </nav>
    </div>
  );
}
