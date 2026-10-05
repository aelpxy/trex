import { useRef, useState, type ReactNode } from "react";
import { Menu } from "@base-ui/react/menu";
import { Link } from "react-router";
import { LuChevronDown, LuLink, LuPencil, LuTrash2 } from "react-icons/lu";

import { NameDialog } from "~/components/ui/name-dialog";
import { dangerMenuItem, focusRing, menuItem, menuSeparator, popup } from "~/components/ui/styles";
import { MoveToProject } from "~/components/workspace/move-to-project";
import type { Project } from "~/lib/workspace";

type ChatHeaderProps = { title: string; chatId?: string; project?: Project; onRename: (title: string) => void; onDelete?: () => void; actions?: ReactNode };

export function ChatHeader({ title, chatId, project, onRename, onDelete, actions }: ChatHeaderProps) {
  const [renaming, setRenaming] = useState(false);
  // the menu returns focus to its trigger as it closes, which would pull it out of a dialog opened any sooner
  const renameRequested = useRef(false);

  return (
    <header className="glass sticky top-0 z-10 flex h-12 shrink-0 items-center px-3">
      {project && (
        <>
          <Link to={`/projects/${project.id}`} className={`hidden max-w-48 shrink-0 truncate rounded-md px-2 py-1 text-sm text-muted sm:block transition-colors hover:bg-subtle hover:text-ink ${focusRing}`}>
            {project.name}
          </Link>
          <span className="hidden text-sm text-muted/60 sm:inline">/</span>
        </>
      )}
      <Menu.Root
        onOpenChangeComplete={(open) => {
          if (open || !renameRequested.current) return;
          renameRequested.current = false;
          setRenaming(true);
        }}
      >
        <Menu.Trigger className={`flex h-8 max-w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-md px-2 text-sm font-medium transition-colors hover:bg-subtle ${focusRing} data-popup-open:bg-subtle`}>
          <span className="truncate">{title}</span>
          <LuChevronDown size={14} className="shrink-0 text-muted" />
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Positioner side="bottom" align="start" sideOffset={6} className="z-50">
            <Menu.Popup className={`w-48 rounded-lg p-1 ${popup}`}>
              <Menu.Item onClick={() => (renameRequested.current = true)} className={menuItem}><LuPencil size={14} />Rename</Menu.Item>
              <Menu.Item onClick={() => navigator.clipboard.writeText(window.location.href).catch((error) => console.warn("could not copy link", error))} className={menuItem}><LuLink size={14} />Copy link</Menu.Item>
              {chatId && <MoveToProject chatId={chatId} />}
              {onDelete && (
                <>
                  <Menu.Separator className={menuSeparator} />
                  <Menu.Item onClick={onDelete} className={dangerMenuItem}><LuTrash2 size={14} />Delete</Menu.Item>
                </>
              )}
            </Menu.Popup>
          </Menu.Positioner>
        </Menu.Portal>
      </Menu.Root>
      {renaming && (
        <NameDialog
          title="Rename chat"
          description="The new title shows in the sidebar and at the top of the chat."
          label="Title"
          initial={title}
          action="Rename"
          onClose={() => setRenaming(false)}
          onSubmit={(next) => {
            setRenaming(false);
            onRename(next);
          }}
        />
      )}
      {actions && <div className="ml-auto flex shrink-0 items-center pl-2">{actions}</div>}
    </header>
  );
}
