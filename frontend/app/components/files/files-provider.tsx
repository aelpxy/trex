import { createContext, use, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { DeleteConfirmDialog } from "~/components/ui/delete-confirm-dialog";
import { download, putSandboxFile, sandboxFile } from "~/lib/api";
import { trex } from "~/lib/trex";

export type FileStatus = "new" | "edited" | null;

// a file with no text to edit, such as an image, shown from an object url
export type BinaryFile = { url: string; type: string };

type Files = {
  panelOpen: boolean;
  openPath: string | null;
  paths: string[];
  // the chat has a sandbox to browse, even before its files are listed
  canBrowse: boolean;
  loading: boolean;
  error: string | null;
  showPanel: () => void;
  open: (path: string) => void;
  openLibrary: (path: string) => Promise<void>;
  refresh: () => void;
  // files the agent is writing right now
  writing: string[];
  close: () => void;
  read: (path: string) => { content: string; status: FileStatus; original?: string; binary?: BinaryFile };
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

// edits are saved to the sandbox once typing pauses
const SAVE_DELAY_MS = 600;
const TEXT_TYPE = /^text\/|json|javascript|xml|svg|yaml|toml/;

const errorText = (error: unknown) => (error instanceof Error ? error.message : String(error));

type FilesProviderProps = {
  sessionId?: string;
  // the files tool calls wrote, live while the agent works
  generated: Record<string, string>;
  writing: string[];
  // a finished run may have changed anything in the sandbox
  running: boolean;
  children: ReactNode;
};

// the session's sandbox files, plus what the agent's tool calls show before the sandbox is read;
// user edits show on top of the agent's version and are saved to the sandbox
export function FilesProvider({ sessionId, generated, writing, running, children }: FilesProviderProps) {
  const [panelOpen, setPanelOpen] = useState(false);
  const [openPath, setOpenPath] = useState<string | null>(null);
  const [listed, setListed] = useState<string[]>([]);
  const [loading, setLoading] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const [overrides, setOverrides] = useState<Record<string, string | null>>({});
  const [pendingRemoval, setPendingRemoval] = useState<string | null>(null);
  // contents read from the sandbox or the library, newer than the tool calls that wrote them
  const [loaded, setLoaded] = useState<Record<string, string>>({});
  const [binaries, setBinaries] = useState<Record<string, BinaryFile>>({});
  const saveTimers = useRef(new Map<string, ReturnType<typeof setTimeout>>());

  const previous = useRef(generated);
  const latest = useRef(generated);
  latest.current = generated;
  // once the agent writes a file again, its version is newer than what was read, and replaces the user's edit on disk
  useEffect(() => {
    const changed = Object.keys(generated).filter((path) => generated[path] !== previous.current[path]);
    previous.current = generated;
    if (!changed.length) return;
    const keep = <T,>(record: Record<string, T>) => Object.fromEntries(Object.entries(record).filter(([path]) => !changed.includes(path)));
    setLoaded(keep);
    setOverrides(keep);
  }, [generated]);
  const agentFiles = useMemo(() => ({ ...generated, ...loaded }), [generated, loaded]);

  const paths = useMemo(
    () => [...new Set([...listed, ...Object.keys(agentFiles), ...Object.keys(overrides)])].filter((path) => overrides[path] !== null).sort(),
    [listed, agentFiles, overrides],
  );

  const refresh = useCallback(() => {
    if (!sessionId) return;
    setLoading(true);
    trex
      .sandboxFiles(sessionId)
      .then((list) => {
        setListed(list.data.map((file) => file.path));
        setError(list.has_more ? "Only the first 5000 files are shown." : null);
      })
      .catch((cause) => setError(`Couldn't list the sandbox files. ${errorText(cause)}`))
      .finally(() => setLoading(false));
  }, [sessionId]);

  // listing starts a stopped sandbox, so it waits until the panel is opened
  useEffect(() => {
    if (panelOpen) refresh();
  }, [panelOpen, refresh]);

  const wasRunning = useRef(running);
  useEffect(() => {
    if (wasRunning.current && !running && panelOpen) refresh();
    wasRunning.current = running;
  }, [running, panelOpen, refresh]);

  const load = useCallback(
    async (path: string) => {
      if (!sessionId || writing.includes(path)) return;
      // a read that raced the agent writing the file would show the older version
      const before = latest.current[path];
      try {
        const blob = await sandboxFile(sessionId, path);
        if (latest.current[path] !== before) return;
        if (TEXT_TYPE.test(blob.type)) {
          const content = await blob.text();
          setLoaded((current) => ({ ...current, [path]: content }));
        } else {
          setBinaries((current) => ({ ...current, [path]: { url: URL.createObjectURL(blob), type: blob.type } }));
        }
      } catch (cause) {
        // files only seen in tool calls may be gone from the sandbox; their tool call version still shows
        console.warn("could not read the sandbox file", cause);
      }
    },
    [sessionId, writing],
  );

  const read = useCallback(
    (path: string) => {
      const override = overrides[path];
      const original = agentFiles[path];
      const binary = binaries[path];
      if (typeof override !== "string") return { content: original ?? "", status: null, original, binary };
      return { content: override, status: original === undefined ? "new" : override === original ? null : "edited", original, binary } as const;
    },
    [agentFiles, overrides, binaries],
  );

  const save = useCallback(
    (path: string, content: string, delay = SAVE_DELAY_MS) => {
      if (!sessionId) return;
      clearTimeout(saveTimers.current.get(path));
      saveTimers.current.set(
        path,
        setTimeout(() => {
          saveTimers.current.delete(path);
          putSandboxFile(sessionId, path, content).catch((cause) => setError(`Couldn't save ${path}. ${errorText(cause)}`));
        }, delay),
      );
    },
    [sessionId],
  );

  useEffect(() => {
    const timers = saveTimers.current;
    return () => timers.forEach((timer) => clearTimeout(timer));
  }, []);

  const showPanel = useCallback(() => setPanelOpen(true), []);
  const close = useCallback(() => setPanelOpen(false), []);
  const open = useCallback(
    (path: string) => {
      setOpenPath(path);
      setPanelOpen(true);
      void load(path);
    },
    [load],
  );
  const openLibrary = useCallback(async (path: string) => {
    const content = await (await download(path)).text();
    setLoaded((current) => ({ ...current, [path]: content }));
    setOpenPath(path);
    setPanelOpen(true);
  }, []);

  const write = useCallback(
    (path: string, content: string) => {
      setOverrides((current) => ({ ...current, [path]: content }));
      save(path, content);
    },
    [save],
  );

  const revert = useCallback(
    (path: string) => {
      setOverrides((current) => {
        const { [path]: _discarded, ...rest } = current;
        return rest;
      });
      const original = agentFiles[path];
      if (original !== undefined) save(path, original, 0);
    },
    [agentFiles, save],
  );

  const create = useCallback(
    (path: string) => {
      setOverrides((current) => ({ ...current, [path]: "" }));
      setOpenPath(path);
      save(path, "", 0);
    },
    [save],
  );

  const rename = useCallback(
    (from: string, to: string) => {
      const { content } = read(from);
      setOverrides((current) => ({ ...current, [to]: content, [from]: null }));
      setOpenPath((current) => (current === from ? to : current));
      if (!sessionId) return;
      trex
        .moveSandboxFile(sessionId, from, to)
        .then(() => setListed((current) => [...current.filter((path) => path !== from), to]))
        // a file the user made but that isn't saved yet only exists here
        .catch((cause) => (from in overrides && !listed.includes(from) ? save(to, content, 0) : setError(`Couldn't move ${from}. ${errorText(cause)}`)));
    },
    [read, sessionId, overrides, listed, save],
  );

  const remove = (path: string) => {
    setOverrides((current) => ({ ...current, [path]: null }));
    setOpenPath((current) => (current === path ? null : current));
    setPendingRemoval(null);
    if (!sessionId) return;
    clearTimeout(saveTimers.current.get(path));
    trex
      .deleteSandboxFile(sessionId, path)
      .then(() => setListed((current) => current.filter((existing) => existing !== path)))
      .catch((cause) => setError(`Couldn't delete ${path}. ${errorText(cause)}`));
  };

  const value = useMemo(
    () => ({
      panelOpen,
      openPath,
      paths,
      canBrowse: Boolean(sessionId),
      loading,
      error,
      showPanel,
      open,
      openLibrary,
      refresh,
      writing,
      close,
      read,
      write,
      revert,
      create,
      rename,
      requestRemove: setPendingRemoval,
    }),
    [panelOpen, openPath, paths, sessionId, loading, error, showPanel, open, openLibrary, refresh, writing, close, read, write, revert, create, rename],
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
