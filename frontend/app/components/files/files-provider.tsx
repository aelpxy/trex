import { createContext, use, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { DeleteConfirmDialog } from "~/components/ui/delete-confirm-dialog";
import { download } from "~/lib/api";

export type FileStatus = "new" | "edited" | null;

type Files = {
  panelOpen: boolean;
  openPath: string | null;
  paths: string[];
  showPanel: () => void;
  open: (path: string) => void;
  openLibrary: (path: string) => Promise<void>;
  // files the agent is writing right now
  writing: string[];
  close: () => void;
  read: (path: string) => { content: string; status: FileStatus; original?: string };
  revert: (path: string) => void;
  write: (path: string, content: string) => void;
  create: (path: string) => void;
  rename: (from: string, to: string) => void;
  requestRemove: (path: string) => void;
};

const FilesContext = createContext<Files | null>(null);

// for components that also render outside a chat, such as markdown
export function useOptionalFiles() {
  return use(FilesContext);
}

export function useFiles() {
  const value = use(FilesContext);
  if (!value) throw new Error("useFiles must be used inside FilesProvider");
  return value;
}

export function normalizePath(path: string) {
  return path.trim().replace(/^\/+|\/+$/g, "").replace(/\/{2,}/g, "/");
}

// user changes sit on top of the agent's files in memory until the trex api can write to the sandbox; null marks a deleted file
export function FilesProvider({ generated, writing, children }: { generated: Record<string, string>; writing: string[]; children: ReactNode }) {
  const [panelOpen, setPanelOpen] = useState(false);
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [overrides, setOverrides] = useState<Record<string, string | null>>({});
  const [pendingRemoval, setPendingRemoval] = useState<string | null>(null);
  const [saved, setSaved] = useState<Record<string, string>>({});
  // a library file the reply links to is the saved deliverable, so it wins over the agent's earlier writes
  const previous = useRef(generated);
  // once the agent writes a file again, its version is newer than the library copy
  useEffect(() => {
    const changed = Object.keys(generated).filter((path) => generated[path] !== previous.current[path]);
    previous.current = generated;
    if (changed.length) setSaved((current) => Object.fromEntries(Object.entries(current).filter(([path]) => !changed.includes(path))));
  }, [generated]);
  const agentFiles = useMemo(() => ({ ...generated, ...saved }), [generated, saved]);

  const paths = useMemo(
    () => [...new Set([...Object.keys(agentFiles), ...Object.keys(overrides)])].filter((path) => overrides[path] !== null).sort(),
    [agentFiles, overrides],
  );

  const read = useCallback(
    (path: string) => {
      const override = overrides[path];
      const original = agentFiles[path];
      if (typeof override !== "string") return { content: original ?? "", status: null, original };
      return { content: override, status: original === undefined ? "new" : override === original ? null : "edited", original } as const;
    },
    [agentFiles, overrides],
  );

  const showPanel = useCallback(() => setPanelOpen(true), []);
  const close = useCallback(() => setPanelOpen(false), []);
  const open = useCallback((path: string) => {
    setOpenPath(path);
    setPanelOpen(true);
  }, []);
  const openLibrary = useCallback(async (path: string) => {
    const content = await (await download(path)).text();
    setSaved((current) => ({ ...current, [path]: content }));
    setOpenPath(path);
    setPanelOpen(true);
  }, []);
  const write = useCallback((path: string, content: string) => setOverrides((current) => ({ ...current, [path]: content })), []);
  const revert = useCallback(
    (path: string) =>
      setOverrides((current) => {
        const { [path]: _discarded, ...rest } = current;
        return rest;
      }),
    [],
  );

  const create = useCallback((path: string) => {
    setOverrides((current) => ({ ...current, [path]: "" }));
    setOpenPath(path);
  }, []);

  const rename = useCallback(
    (from: string, to: string) => {
      const { content } = read(from);
      setOverrides((current) => ({ ...current, [to]: content, [from]: null }));
      setOpenPath((current) => (current === from ? to : current));
    },
    [read],
  );

  const remove = (path: string) => {
    setOverrides((current) => ({ ...current, [path]: null }));
    setOpenPath((current) => (current === path ? null : current));
    setPendingRemoval(null);
  };

  const value = useMemo(
    () => ({ panelOpen, openPath, paths, showPanel, open, openLibrary, writing, close, read, write, revert, create, rename, requestRemove: setPendingRemoval }),
    [panelOpen, openPath, paths, showPanel, open, openLibrary, writing, close, read, write, revert, create, rename],
  );

  return (
    <FilesContext value={value}>
      {children}
      <DeleteConfirmDialog
        target={pendingRemoval ? { kind: "file", id: pendingRemoval, name: pendingRemoval } : null}
        onConfirm={(target) => remove(target.id)}
        onCancel={() => setPendingRemoval(null)}
      />
    </FilesContext>
  );
}
