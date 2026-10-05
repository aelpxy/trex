import { Collapsible } from "@base-ui/react/collapsible";
import { LuChevronRight, LuLoaderCircle, LuMinimize2 } from "react-icons/lu";

import { Markdown } from "~/components/markdown/markdown";
import { collapsiblePanel, focusRing } from "~/components/ui/styles";

import type { CompactionMessage } from "../types";

const LABEL = { running: "Summarizing context…", done: "Context summarized", stopped: "Summarizing stopped", failed: "Couldn't summarize the context" };

// a compaction the user asked for, between turns: a quiet rule that opens to the summary
export function CompactionDivider({ message }: { message: CompactionMessage }) {
  const line = <span aria-hidden className="h-px flex-1 bg-line" />;
  const label = (
    <>
      {message.state === "running" ? <LuLoaderCircle size={13} className="animate-spin" /> : <LuMinimize2 size={13} />}
      <span className={message.state === "failed" ? "text-danger" : undefined}>{LABEL[message.state]}</span>
    </>
  );

  if (!message.summary) {
    return (
      <div role="status" className="flex items-center gap-3 text-xs text-muted">
        {line}
        <span className="flex items-center gap-2" title={message.error}>
          {label}
        </span>
        {line}
      </div>
    );
  }
  return (
    <Collapsible.Root>
      <div className="flex items-center gap-3 text-xs text-muted">
        {line}
        <Collapsible.Trigger className={`group flex cursor-pointer items-center gap-2 rounded-md px-1 hover:text-ink ${focusRing}`}>
          {label}
          <LuChevronRight size={12} className="transition-transform duration-150 group-data-panel-open:rotate-90" />
        </Collapsible.Trigger>
        {line}
      </div>
      <Collapsible.Panel className={collapsiblePanel}>
        <div className="mt-3 rounded-xl border border-line bg-surface/60 px-4 py-3 text-sm text-muted">
          <p className="mb-2 text-xs font-medium text-ink">What the agent carries forward</p>
          <Markdown>{message.summary}</Markdown>
        </div>
      </Collapsible.Panel>
    </Collapsible.Root>
  );
}
