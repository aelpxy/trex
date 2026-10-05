import { Collapsible } from "@base-ui/react/collapsible";
import { LuFolder, LuFolderOpen } from "react-icons/lu";

import { collapsiblePanel } from "~/components/ui/styles";
import type { Project } from "~/lib/workspace";
import { toggleItem, useUiState } from "~/lib/ui-state";

import { ChatLink } from "./chat-link";
import { ItemContextMenu } from "./item-context-menu";
import { sectionItem } from "./styles";

type ProjectFolderProps = { project: Project; onNavigate?: () => void };

export function ProjectFolder({ project, onNavigate }: ProjectFolderProps) {
  const { state, update } = useUiState();

  return (
    <Collapsible.Root
      open={state.openProjects.includes(project.id)}
      onOpenChange={(open) => update((current) => ({ openProjects: toggleItem(current.openProjects, project.id, open) }))}
    >
      <ItemContextMenu target={{ kind: "project", id: project.id, name: project.name }}>
        <Collapsible.Trigger className={`group ${sectionItem}`}>
          <LuFolder size={15} className="shrink-0 group-data-panel-open:hidden" />
          <LuFolderOpen size={15} className="hidden shrink-0 group-data-panel-open:block" />
          <span className="truncate">{project.name}</span>
        </Collapsible.Trigger>
      </ItemContextMenu>
      <Collapsible.Panel className={collapsiblePanel}>
        <ul className="ml-[1.2rem] space-y-0.5 border-l border-line py-0.5 pl-1.5">
          {project.chats.map((chat) => (
            <li key={chat.id}>
              <ChatLink chat={chat} onNavigate={onNavigate} />
            </li>
          ))}
          {project.chats.length === 0 && <li className="px-3 py-1.5 text-xs text-muted">No chats yet</li>}
        </ul>
      </Collapsible.Panel>
    </Collapsible.Root>
  );
}
