import type { ReactNode } from "react";
import { Collapsible } from "@base-ui/react/collapsible";
import { LuChevronRight } from "react-icons/lu";

import { collapsiblePanel, focusRing } from "~/components/ui/styles";
import { toggleItem, useUiState } from "~/lib/ui-state";

import { useChatDrop } from "./chat-drag";

// `drop` makes the whole section a place to drop chats on
type SidebarSectionProps = { id: string; title: string; action?: ReactNode; drop?: (chatId: string) => void; children: ReactNode };

export function SidebarSection({ id, title, action, drop, children }: SidebarSectionProps) {
  const { state, update } = useUiState();
  const target = useChatDrop((chatId) => drop?.(chatId));

  return (
    <Collapsible.Root
      {...(drop && target.props)}
      className={`rounded-md transition-shadow ${drop && target.over ? "bg-subtle/60 ring-1 ring-accent" : ""}`}
      open={!state.closedSections.includes(id)}
      onOpenChange={(open) => update((current) => ({ closedSections: toggleItem(current.closedSections, id, !open) }))}
    >
      <div className="group/section flex items-center gap-0.5">
        <Collapsible.Trigger className={`group flex h-7 min-w-0 flex-1 cursor-pointer items-center gap-1.5 rounded-md px-3 text-xs font-medium text-muted transition-colors hover:text-ink ${focusRing}`}>
          <span className="truncate">{title}</span>
          <LuChevronRight size={13} className="shrink-0 transition-transform duration-150 group-data-panel-open:rotate-90" />
        </Collapsible.Trigger>
        {action}
      </div>
      <Collapsible.Panel className={collapsiblePanel}>
        <ul className="space-y-0.5 pt-0.5">{children}</ul>
      </Collapsible.Panel>
    </Collapsible.Root>
  );
}
