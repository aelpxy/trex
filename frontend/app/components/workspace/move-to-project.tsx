import { Menu } from "@base-ui/react/menu";
import { LuCheck, LuChevronRight, LuFolder, LuFolderInput, LuFolderMinus } from "react-icons/lu";

import { menuItem, menuSeparator, popup } from "~/components/ui/styles";

import { useWorkspace } from "./workspace-provider";

// works in menus and context menus alike, since they share their parts
export function MoveToProject({ chatId }: { chatId: string }) {
  const { projects, recents, moveChat } = useWorkspace();
  const current = recents.some((chat) => chat.id === chatId) ? null : (projects.find((project) => project.chats.some((chat) => chat.id === chatId))?.id ?? null);

  return (
    <Menu.SubmenuRoot>
      <Menu.SubmenuTrigger className={`${menuItem} data-popup-open:bg-subtle data-popup-open:text-ink`}>
        <LuFolderInput size={14} />
        Move to project
        <LuChevronRight size={14} className="ml-auto" />
      </Menu.SubmenuTrigger>
      <Menu.Portal>
        <Menu.Positioner className="z-50" sideOffset={4}>
          <Menu.Popup className={`max-h-72 w-52 overflow-y-auto rounded-lg p-1 ${popup}`}>
            {projects.length === 0 && <div className="px-2.5 py-2 text-[13px] text-muted">No projects yet</div>}
            {projects.map((project) => (
              <Menu.Item key={project.id} disabled={project.id === current} onClick={() => moveChat(chatId, project.id)} className={menuItem}>
                <LuFolder size={14} className="shrink-0" />
                <span className="truncate">{project.name}</span>
                {project.id === current && <LuCheck size={14} className="ml-auto shrink-0" />}
              </Menu.Item>
            ))}
            {current && (
              <>
                <Menu.Separator className={menuSeparator} />
                <Menu.Item onClick={() => moveChat(chatId, null)} className={menuItem}>
                  <LuFolderMinus size={14} />
                  Remove from project
                </Menu.Item>
              </>
            )}
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.SubmenuRoot>
  );
}
