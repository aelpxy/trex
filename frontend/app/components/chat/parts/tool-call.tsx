import { Collapsible } from "@base-ui/react/collapsible";
import { useMemo } from "react";
import { Button } from "@base-ui/react/button";
import { LuChevronRight, LuCircleCheck, LuCircleX, LuFileCode, LuFilePen, LuLoaderCircle, LuPanelRightOpen, LuSquareTerminal } from "react-icons/lu";

import { diffRows, diffStats } from "~/components/diff/diff";
import { DiffView } from "~/components/diff/diff-view";
import { useFiles } from "~/components/files/files-provider";

import { CodeBlock } from "~/components/markdown/code-block";
import { collapsiblePanel, focusRing, iconButton } from "~/components/ui/styles";

import type { ToolPart } from "../types";
import { formatDuration, languageOf } from "./format";


const STATE_LABEL = { running: "Running", done: "Succeeded", error: "Failed" };
const TOOL_LABEL = { shell: "Run", write_file: "Write", edit_file: "Edit" };
const TOOL_ICON = { shell: LuSquareTerminal, write_file: LuFileCode, edit_file: LuFilePen };

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
  const Icon = TOOL_ICON[part.name];
  const { open } = useFiles();

  return (
    <Collapsible.Root className="overflow-hidden rounded-xl border border-line bg-surface/60">
      <div className="flex items-center">
        <Collapsible.Trigger className={`group flex h-10 min-w-0 flex-1 cursor-pointer items-center gap-2.5 px-3 text-left text-xs transition-colors hover:bg-subtle/60 ${focusRing}`}>
          <Icon size={15} className="shrink-0 text-muted" />
          <span className="shrink-0 font-medium">{TOOL_LABEL[part.name]}</span>
          <code className="min-w-0 flex-1 truncate font-mono text-[11px] text-muted">{shell ? part.input.command : part.input.path}</code>
          {part.summary && <span className="shrink-0 font-mono text-[11px] text-muted">{part.summary}</span>}
          {edit && part.input.path && <EditSummary path={part.input.path} before={part.input.before ?? ""} after={part.input.after ?? ""} />}
          {part.endedAt && <span className="shrink-0 text-[11px] text-muted tabular-nums">{formatDuration(part.endedAt - part.startedAt)}</span>}
          <span className="sr-only">{STATE_LABEL[part.state]}</span>
          <StateIcon state={part.state} />
          <LuChevronRight size={13} className="shrink-0 text-muted transition-transform duration-150 group-data-panel-open:rotate-90" />
        </Collapsible.Trigger>
        {!shell && part.input.path && (
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
          ) : (
            <div className="p-2">
              <CodeBlock code={part.output.replace(/\n$/, "")} language={languageOf(part.input.path)} lineNumbers />
            </div>
          )}
        </div>
      </Collapsible.Panel>
    </Collapsible.Root>
  );
}
