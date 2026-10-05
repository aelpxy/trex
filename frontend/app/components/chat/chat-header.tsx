import { useState, type KeyboardEvent, type ReactNode } from "react";
import { Menu } from "@base-ui/react/menu";
import { LuChevronDown, LuLink, LuPencil, LuTrash2 } from "react-icons/lu";

import { dangerMenuItem, focusRing, menuItem, menuSeparator, popup } from "~/components/ui/styles";

type ChatHeaderProps = { title: string; onRename: (title: string) => void; onDelete?: () => void; actions?: ReactNode };

export function ChatHeader({ title, onRename, onDelete, actions }: ChatHeaderProps) {
  const [editing, setEditing] = useState(false);

  function commit(value: string) {
    const next = value.trim();
    if (next && next !== title) onRename(next);
    setEditing(false);
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Enter") commit(event.currentTarget.value);
    if (event.key === "Escape") setEditing(false);
  }

  return (
    <header className="glass sticky top-0 z-10 flex h-12 shrink-0 items-center px-3">
      {editing ? (
        <input
          autoFocus
          defaultValue={title}
          aria-label="Chat title"
          onFocus={(event) => event.currentTarget.select()}
          onBlur={(event) => commit(event.currentTarget.value)}
          onKeyDown={onKeyDown}
          className="h-8 w-full max-w-sm rounded-md bg-surface px-2 text-sm font-medium ring-1 ring-line outline-none focus:ring-muted/50"
        />
      ) : (
        <Menu.Root>
          <Menu.Trigger className={`flex h-8 max-w-full min-w-0 cursor-pointer items-center gap-1.5 rounded-md px-2 text-sm font-medium transition-colors hover:bg-subtle ${focusRing} data-popup-open:bg-subtle`}>
            <span className="truncate">{title}</span>
            <LuChevronDown size={14} className="shrink-0 text-muted" />
          </Menu.Trigger>
          <Menu.Portal>
            <Menu.Positioner side="bottom" align="start" sideOffset={6} className="z-50">
              <Menu.Popup className={`w-48 rounded-lg p-1 ${popup}`}>
                <Menu.Item onClick={() => setEditing(true)} className={menuItem}><LuPencil size={14} />Rename</Menu.Item>
                <Menu.Item onClick={() => navigator.clipboard.writeText(window.location.href).catch((error) => console.warn("could not copy link", error))} className={menuItem}><LuLink size={14} />Copy link</Menu.Item>
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
      )}
      {actions && <div className="ml-auto flex shrink-0 items-center pl-2">{actions}</div>}
    </header>
  );
}
