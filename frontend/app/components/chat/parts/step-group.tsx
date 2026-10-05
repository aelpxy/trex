import { Collapsible } from "@base-ui/react/collapsible";
import type { ReactNode } from "react";
import { LuChevronRight } from "react-icons/lu";

import { collapsiblePanel, focusRing } from "~/components/ui/styles";

import type { Part } from "../types";

const plural = (count: number, one: string, many: string) => `${count} ${count === 1 ? one : many}`;

// what a run of steps did, e.g. "Ran 3 commands, edited 2 files"
export function summarize(parts: Part[]) {
  const tools = parts.flatMap((part) => (part.type === "tool" ? [part] : []));
  const count = (name: string) => tools.filter((tool) => tool.name === name).length;
  const edited = new Set(tools.filter((tool) => tool.name === "write_file" || tool.name === "edit_file").map((tool) => tool.input.path)).size;
  const phrases = [
    count("shell") && `ran ${plural(count("shell"), "command", "commands")}`,
    edited && `edited ${plural(edited, "file", "files")}`,
    count("read_file") && `read ${plural(count("read_file"), "file", "files")}`,
    count("search") && plural(count("search"), "search", "searches"),
    count("web") && `fetched ${plural(count("web"), "page", "pages")}`,
    count("browse") && `checked ${plural(count("browse"), "page", "pages")} in the browser`,
    count("image") && `viewed ${plural(count("image"), "image", "images")}`,
    count("library") && "used the library",
    count("process") && `checked ${plural(count("process"), "process", "processes")}`,
  ].filter(Boolean) as string[];
  const text = phrases.length ? phrases.join(", ") : tools.length ? plural(tools.length, "step", "steps") : "thought it through";
  return text.charAt(0).toUpperCase() + text.slice(1);
}

export function StepGroup({ parts, children }: { parts: Part[]; children: ReactNode }) {
  const failed = parts.some((part) => part.type === "tool" && part.state === "error");
  return (
    <Collapsible.Root>
      <Collapsible.Trigger className={`group flex cursor-pointer items-center gap-1.5 rounded-md text-xs text-muted transition-colors hover:text-ink ${focusRing}`}>
        <span>{summarize(parts)}</span>
        {failed && <span className="text-danger">· some failed</span>}
        <LuChevronRight size={13} className="shrink-0 transition-transform duration-150 group-data-panel-open:rotate-90" />
      </Collapsible.Trigger>
      <Collapsible.Panel className={collapsiblePanel}>
        <div className="mt-2 space-y-3 border-l border-line pl-3">{children}</div>
      </Collapsible.Panel>
    </Collapsible.Root>
  );
}
