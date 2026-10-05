import { Collapsible } from "@base-ui/react/collapsible";
import { useEffect, useMemo, useState } from "react";
import { Button } from "@base-ui/react/button";
import { LuActivity, LuAppWindow, LuChevronRight, LuCircleCheck, LuCircleX, LuClock, LuFileCode, LuFilePen, LuFileText, LuGlobe, LuImage, LuLibrary, LuLoaderCircle, LuPanelRightOpen, LuSearch, LuSquareTerminal } from "react-icons/lu";

import { diffRows, diffStats } from "~/components/diff/diff";
import { DiffView } from "~/components/diff/diff-view";
import { useFiles } from "~/components/files/files-provider";
import { sandboxFile } from "~/lib/api";

import { CodeBlock } from "~/components/markdown/code-block";
import { collapsiblePanel, focusRing, iconButton } from "~/components/ui/styles";

import type { ToolPart } from "../types";
import { AttachmentImage, imagesIn, withoutImages } from "./attachment-image";
import { formatDuration, languageOf } from "./format";


const STATE_LABEL = { running: "Running", done: "Succeeded", error: "Failed" };
const TOOL_LABEL = {
  shell: "Run",
  write_file: "Write",
  edit_file: "Edit",
  read_file: "Read",
  search: "Search",
  web: "Fetch",
  browse: "Browse",
  library: "Library",
  image: "View image",
  process: "Check process",
  time: "Check time",
};
const TOOL_ICON = {
  shell: LuSquareTerminal,
  write_file: LuFileCode,
  edit_file: LuFilePen,
  read_file: LuFileText,
  search: LuSearch,
  web: LuGlobe,
  browse: LuAppWindow,
  library: LuLibrary,
  image: LuImage,
  process: LuActivity,
  time: LuClock,
};
const FILE_TOOLS = new Set(["write_file", "edit_file", "read_file", "image"]);

// read_file numbers its lines for the model, and notes when it stopped early
const fileText = (output: string) =>
  output
    .replace(/\n?\[showing lines[^\]]*\]\s*$/, "")
    .split("\n")
    .map((line) => line.replace(/^\s*\d+\t/, ""))
    .join("\n")
    .replace(/\n$/, "");

function SandboxImage({ path }: { path: string }) {
  const { sessionId } = useFiles();
  const [url, setUrl] = useState<string>();
  useEffect(() => {
    if (!sessionId) return;
    let current: string | undefined;
    let cancelled = false;
    sandboxFile(sessionId, path)
      .then((blob) => {
        if (cancelled) return;
        current = URL.createObjectURL(blob);
        setUrl(current);
      })
      .catch((error) => console.warn("could not load the image", error));
    return () => {
      cancelled = true;
      if (current) URL.revokeObjectURL(current);
    };
  }, [sessionId, path]);
  if (!url) return <p className="p-3 text-xs text-muted">{sessionId ? "Loading image…" : path}</p>;
  return (
    <div className="flex justify-center p-3">
      <img src={url} alt={path} className="max-h-80 rounded-lg border border-line" />
    </div>
  );
}

function EditSummary({ path, before, after }: { path: string; before: string; after: string }) {
  const { added, removed } = useMemo(() => diffStats(diffRows(path, before, after)), [path, before, after]);
  return (
    <span className="shrink-0 font-mono text-[11px] tabular-nums">
      <span className="text-diff-add-fg">+{added}</span> <span className="text-diff-remove-fg">−{removed}</span>
      <span className="sr-only">
        {added} lines added, {removed} removed
      </span>
    </span>
  );
}

function StateIcon({ state }: { state: ToolPart["state"] }) {
  if (state === "running") return <LuLoaderCircle size={14} className="animate-spin text-muted" />;
  if (state === "error") return <LuCircleX size={14} className="text-danger" />;
  return <LuCircleCheck size={14} className="text-muted" />;
}

export function ToolCall({ part }: { part: ToolPart }) {
  const shell = part.name === "shell";
  const edit = part.name === "edit_file";
  const file = FILE_TOOLS.has(part.name);
  const subject = shell ? part.input.command : (part.input.path ?? part.input.detail);
  const Icon = TOOL_ICON[part.name];
  const { open } = useFiles();

  return (
    <Collapsible.Root className="overflow-hidden rounded-xl border border-line bg-surface/60">
      <div className="flex items-center">
        <Collapsible.Trigger className={`group flex h-10 min-w-0 flex-1 cursor-pointer items-center gap-2.5 px-3 text-left text-xs transition-colors hover:bg-subtle/60 ${focusRing}`}>
          <Icon size={15} className="shrink-0 text-muted" />
          <span className="shrink-0 font-medium">{part.title ?? TOOL_LABEL[part.name]}</span>
          <code className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted">{subject}</code>
          {part.summary && <span className="shrink-0 font-mono text-[11px] text-muted">{part.summary}</span>}
          {edit && part.input.path && <EditSummary path={part.input.path} before={part.input.before ?? ""} after={part.input.after ?? ""} />}
          {part.endedAt && <span className="shrink-0 text-[11px] text-muted tabular-nums">{formatDuration(part.endedAt - part.startedAt)}</span>}
          <span className="sr-only">{STATE_LABEL[part.state]}</span>
          <StateIcon state={part.state} />
          <LuChevronRight size={13} className="shrink-0 text-muted transition-transform duration-150 group-data-panel-open:rotate-90" />
        </Collapsible.Trigger>
        {file && part.name !== "image" && part.input.path && (
          <Button onClick={() => open(part.input.path!)} aria-label={`Open ${part.input.path} in editor`} title="Open in editor" className={`${iconButton} mr-1 size-7`}>
            <LuPanelRightOpen size={14} />
          </Button>
        )}
      </div>
      <Collapsible.Panel className={collapsiblePanel}>
        <div className="border-t border-line">
          {shell ? (
            <pre className="max-h-72 overflow-auto p-3 font-mono text-[11px] leading-5">
              <span className="text-muted select-none">$ </span>
              {part.input.command}
              {"\n"}
              <span className={part.state === "error" ? "text-danger" : "text-muted"}>{part.output}</span>
            </pre>
          ) : edit ? (
            <DiffView path={part.input.path ?? ""} before={part.input.before ?? ""} after={part.input.after ?? ""} className="max-h-80 py-1" />
          ) : part.name === "browse" ? (
            <div className="space-y-3 p-3">
              {imagesIn(part.output).map((hash) => (
                <AttachmentImage key={hash} hash={hash} alt={`Screenshot of ${part.input.detail ?? "the page"}`} />
              ))}
              <pre className={`max-h-72 overflow-auto font-mono text-[11px] leading-5 whitespace-pre-wrap ${part.state === "error" ? "text-danger" : "text-muted"}`}>
                {withoutImages(part.output) || (part.state === "running" ? "Opening the page…" : "No output")}
              </pre>
            </div>
          ) : part.name === "image" && part.state === "done" && part.input.path ? (
            <SandboxImage path={part.input.path} />
          ) : file ? (
            <div className="p-2">
              <CodeBlock code={part.name === "read_file" ? fileText(part.output) : part.output.replace(/\n$/, "")} language={languageOf(part.input.path)} lineNumbers />
            </div>
          ) : (
            <pre className={`max-h-72 overflow-auto p-3 font-mono text-[11px] leading-5 whitespace-pre-wrap ${part.state === "error" ? "text-danger" : "text-muted"}`}>
              {part.output.replace(/\n$/, "") || "No output"}
            </pre>
          )}
        </div>
      </Collapsible.Panel>
    </Collapsible.Root>
  );
}
