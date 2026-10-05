import { lazy, Suspense, useEffect, useRef, useState } from "react";
import { Button } from "@base-ui/react/button";
import { Tabs } from "@base-ui/react/tabs";
import { LuCheck, LuCode, LuCopy, LuDownload, LuEye, LuFileDiff, LuUndo2, LuFolderTree, LuPanelLeftClose, LuPanelLeftOpen, LuX } from "react-icons/lu";

import { DiffView } from "~/components/diff/diff-view";
import { Markdown } from "~/components/markdown/markdown";
import { Button as UiButton } from "~/components/ui/button";
import { EmptyState } from "~/components/ui/empty-state";
import { Skeleton } from "~/components/ui/skeleton";
import { focusRing, iconButton } from "~/components/ui/styles";

import { FileTree } from "./file-tree";
import { useFiles } from "./files-provider";

const CodeEditor = lazy(() => import("./code-editor"));

const COPIED_RESET_MS = 1500;
const tab = `flex h-7 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-xs font-medium text-muted transition-colors hover:text-ink data-active:text-ink ${focusRing}`;

type Preview = "markdown" | "html" | null;

function previewOf(path: string): Preview {
  if (/\.(md|markdown)$/i.test(path)) return "markdown";
  if (/\.html?$/i.test(path)) return "html";
  return null;
}

function download(path: string, content: string) {
  const url = URL.createObjectURL(new Blob([content], { type: "text/plain" }));
  const link = Object.assign(document.createElement("a"), { href: url, download: path.split("/").pop() ?? "file" });
  link.click();
  URL.revokeObjectURL(url);
}

function EditorFallback() {
  return (
    <div className="space-y-2 p-4" role="status" aria-label="Loading editor">
      {["w-4/5", "w-2/3", "w-11/12", "w-2/5", "w-3/4"].map((width) => (
        <Skeleton key={width} className={`h-3 ${width}`} />
      ))}
    </div>
  );
}

function FileBody({ path }: { path: string }) {
  const { read, write, revert } = useFiles();
  const { content, status, original } = read(path);
  const preview = previewOf(path);
  const changed = status === "edited" && original !== undefined;
  const editor = (
    <Suspense fallback={<EditorFallback />}>
      <CodeEditor path={path} value={content} onChange={(next) => write(path, next)} />
    </Suspense>
  );

  if (!preview && !changed) return <div className="min-h-0 flex-1">{editor}</div>;

  return (
    <Tabs.Root defaultValue="code" className="flex min-h-0 flex-1 flex-col">
      <Tabs.List aria-label="View" className="relative flex items-center gap-1 border-b border-line px-3 py-1.5">
        <Tabs.Tab value="code" className={tab}>
          <LuCode size={13} />
          Code
        </Tabs.Tab>
        {preview && (
          <Tabs.Tab value="preview" className={tab}>
            <LuEye size={13} />
            Preview
          </Tabs.Tab>
        )}
        {changed && (
          <Tabs.Tab value="changes" className={tab}>
            <LuFileDiff size={13} />
            Changes
          </Tabs.Tab>
        )}
        <Tabs.Indicator className="absolute top-1/2 left-0 -z-1 h-7 w-(--active-tab-width) translate-x-(--active-tab-left) -translate-y-1/2 rounded-md bg-subtle transition-[translate,width] duration-150" />
      </Tabs.List>
      <Tabs.Panel value="code" className="min-h-0 flex-1 outline-none">
        {editor}
      </Tabs.Panel>
      {preview && (
        <Tabs.Panel value="preview" className="min-h-0 flex-1 overflow-auto outline-none">
          {preview === "markdown" ? (
            <div className="p-6">
              <Markdown>{content}</Markdown>
            </div>
          ) : (
            <iframe title={`Preview of ${path}`} sandbox="" srcDoc={content} className="h-full w-full bg-white" />
          )}
        </Tabs.Panel>
      )}
      {changed && (
        <Tabs.Panel value="changes" className="flex min-h-0 flex-1 flex-col outline-none">
          <div className="flex shrink-0 items-center justify-between gap-3 border-b border-line px-4 py-2">
            <p className="text-xs text-muted">Your edits compared with the agent's version.</p>
            <UiButton variant="quiet" onClick={() => revert(path)} className="h-8 px-3 text-xs">
              <LuUndo2 size={13} />
              Discard changes
            </UiButton>
          </div>
          <DiffView path={path} before={original} after={content} className="min-h-0 flex-1 py-1" />
        </Tabs.Panel>
      )}
    </Tabs.Root>
  );
}

const STATUS_LABEL = { new: "New", edited: "Edited" };

export function FilePanel() {
  const { panelOpen, openPath, paths, close, read } = useFiles();
  const [treeOpen, setTreeOpen] = useState(true);
  const [copied, setCopied] = useState(false);
  const closeButton = useRef<HTMLButtonElement>(null);
  const resetTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    if (panelOpen) closeButton.current?.focus();
  }, [panelOpen]);

  useEffect(() => () => {
    if (resetTimer.current) clearTimeout(resetTimer.current);
  }, []);

  if (!panelOpen) return null;
  const file = openPath ? read(openPath) : null;

  async function copy(content: string) {
    try {
      await navigator.clipboard.writeText(content);
      setCopied(true);
      if (resetTimer.current) clearTimeout(resetTimer.current);
      resetTimer.current = setTimeout(() => setCopied(false), COPIED_RESET_MS);
    } catch (error) {
      console.warn("could not copy file", error);
    }
  }

  return (
    <aside
      aria-label="Files"
      onKeyDown={(event) => {
        // menus are portaled and the editor handles its own Escape, so only close for unhandled presses inside the panel
        if (event.key === "Escape" && !event.defaultPrevented && event.currentTarget.contains(event.target as Node)) close();
      }}
      className="glass fixed inset-0 z-40 flex flex-col md:relative md:z-auto md:w-[52%] md:max-w-4xl md:min-w-[32rem] md:border-l md:border-line"
    >
      <header className="flex h-12 shrink-0 items-center gap-2 border-b border-line px-2">
        <Button
          onClick={() => setTreeOpen((value) => !value)}
          aria-label={treeOpen ? "Hide file tree" : "Show file tree"}
          aria-pressed={treeOpen}
          aria-controls="file-tree"
          title={treeOpen ? "Hide file tree" : "Show file tree"}
          className={iconButton}
        >
          {treeOpen ? <LuPanelLeftClose size={15} /> : <LuPanelLeftOpen size={15} />}
        </Button>
        <h2 className="min-w-0 truncate font-mono text-xs" title={openPath ?? undefined}>
          {openPath ?? "Files"}
        </h2>
        {file?.status && <span className="shrink-0 rounded bg-subtle px-1.5 py-0.5 text-[10px] font-medium text-muted">{STATUS_LABEL[file.status]}</span>}
        <div className="ml-auto flex shrink-0 items-center">
          {openPath && file && (
            <>
              <Button onClick={() => copy(file.content)} aria-label={copied ? "Copied" : "Copy file"} title="Copy" className={iconButton}>
                {copied ? <LuCheck size={15} /> : <LuCopy size={15} />}
              </Button>
              <Button onClick={() => download(openPath, file.content)} aria-label="Download file" title="Download" className={iconButton}>
                <LuDownload size={15} />
              </Button>
            </>
          )}
          <Button ref={closeButton} onClick={close} aria-label="Close files" title="Close" className={iconButton}>
            <LuX size={15} />
          </Button>
        </div>
      </header>
      <div className="flex min-h-0 flex-1">
        {treeOpen && (
          <div id="file-tree" className="w-56 shrink-0 border-r border-line">
            <FileTree />
          </div>
        )}
        <div className="flex min-w-0 flex-1 flex-col">
          {openPath ? (
            <FileBody key={openPath} path={openPath} />
          ) : (
            <EmptyState
              icon={LuFolderTree}
              title={paths.length > 0 ? "Select a file" : "No files yet"}
              description={paths.length > 0 ? "Pick a file from the tree to view and edit it." : "Files the agent writes will appear here. You can also create one."}
            />
          )}
        </div>
      </div>
    </aside>
  );
}
