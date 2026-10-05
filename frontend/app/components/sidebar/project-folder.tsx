import { Collapsible } from "@base-ui/react/collapsible";
import { NavLink } from "react-router";
import { LuFolder, LuFolderOpen } from "react-icons/lu";

import { collapsiblePanel, focusRing } from "~/components/ui/styles";
import type { Project } from "~/lib/workspace";
import { toggleItem, useUiState } from "~/lib/ui-state";

import { ChatLink } from "./chat-link";
import { ItemContextMenu } from "./item-context-menu";
import { sectionItem } from "./styles";

type ProjectFolderProps = { project: Project; onNavigate?: () => void };

export function ProjectFolder({ project, onNavigate }: ProjectFolderProps) {
  const { state, update } = useUiState();
  const setOpen = (open: boolean) => update((current) => ({ openProjects: toggleItem(current.openProjects, project.id, open) }));

  return (
    <Collapsible.Root open={state.openProjects.includes(project.id)} onOpenChange={setOpen}>
      <ItemContextMenu target={{ kind: "project", id: project.id, name: project.name }}>
        <div className="relative">
          <NavLink
            to={`/projects/${project.id}`}
            onClick={() => {
              setOpen(true);
              onNavigate?.();
            }}
            className={`${sectionItem} pl-9`}
          >
            <span className="truncate">{project.name}</span>
          </NavLink>
          <Collapsible.Trigger aria-label={`Show chats in ${project.name}`} className={`group absolute top-1 left-1.5 inline-flex size-6 cursor-pointer items-center justify-center rounded-md text-muted transition-colors hover:bg-line/60 hover:text-ink ${focusRing}`}>
            <LuFolder size={15} className="group-data-panel-open:hidden" />
            <LuFolderOpen size={15} className="hidden group-data-panel-open:block" />
          </Collapsible.Trigger>
        </div>
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
