import { Collapsible } from "@base-ui/react/collapsible";
import { LuChevronRight, LuSparkles } from "react-icons/lu";

import { Markdown } from "~/components/markdown/markdown";
import { collapsiblePanel, focusRing } from "~/components/ui/styles";

import type { ReasoningPart } from "../types";
import { formatDuration } from "./format";


export function ReasoningBlock({ part }: { part: ReasoningPart }) {
  const live = part.endedAt === undefined;

  return (
    <Collapsible.Root>
      <Collapsible.Trigger className={`group -mx-1.5 flex cursor-pointer items-center gap-1.5 rounded-md px-1.5 py-0.5 text-xs text-muted transition-colors hover:text-ink ${focusRing}`}>
        <LuSparkles size={13} className={live ? "animate-[think-pulse_1.6s_ease-in-out_infinite]" : ""} />
        {live ? <span className="thinking-label">Thinking</span> : <span>Thought for {formatDuration(part.endedAt! - part.startedAt)}</span>}
        <LuChevronRight size={12} className="transition-transform duration-150 group-data-panel-open:rotate-90" />
      </Collapsible.Trigger>
      <Collapsible.Panel className={collapsiblePanel}>
        <div className="mt-2 border-l-2 border-line pl-3">
          <Markdown className="text-[13px] leading-6 text-muted">{part.text}</Markdown>
        </div>
      </Collapsible.Panel>
    </Collapsible.Root>
  );
}
