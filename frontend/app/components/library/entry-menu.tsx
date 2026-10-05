import { Menu } from "@base-ui/react/menu";
import { LuDownload, LuEllipsis, LuExternalLink, LuFolderInput, LuFolderOpen, LuPencil, LuTrash2 } from "react-icons/lu";

import { dangerMenuItem, iconButton, menuItem, menuSeparator, popup } from "~/components/ui/styles";
import { plural } from "~/lib/format";

import { viewable, type Entry } from "./entries";

export type EntryActions = {
  onOpenFolder: (entry: Entry) => void;
  onShow: (entry: Entry, download: boolean) => void;
  onRename: (entry: Entry) => void;
  onMove: (entries: Entry[]) => void;
  onDelete: (entries: Entry[]) => void;
};

// what can be done to one entry, or to several at once; shared by the row menu and right-click
export function EntryMenuItems({ targets, onOpenFolder, onShow, onRename, onMove, onDelete }: EntryActions & { targets: Entry[] }) {
  const [entry] = targets;
  if (targets.length > 1) {
    return (
      <>
        <Menu.Item onClick={() => onMove(targets)} className={menuItem}>
          <LuFolderInput size={14} />
          Move {plural(targets.length, "item")} to…
        </Menu.Item>
        <Menu.Separator className={menuSeparator} />
        <Menu.Item onClick={() => onDelete(targets)} className={dangerMenuItem}>
          <LuTrash2 size={14} />
          Delete {plural(targets.length, "item")}
        </Menu.Item>
      </>
    );
  }
  return (
    <>
      {entry.kind === "folder" ? (
        <Menu.Item onClick={() => onOpenFolder(entry)} className={menuItem}>
          <LuFolderOpen size={14} />
          Open
        </Menu.Item>
      ) : (
        <>
          {viewable(entry.name) && (
            <Menu.Item onClick={() => onShow(entry, false)} className={menuItem}>
              <LuExternalLink size={14} />
              Open in a new tab
            </Menu.Item>
          )}
          <Menu.Item onClick={() => onShow(entry, true)} className={menuItem}>
            <LuDownload size={14} />
            Download
          </Menu.Item>
        </>
      )}
      <Menu.Separator className={menuSeparator} />
      <Menu.Item onClick={() => onRename(entry)} className={menuItem}>
        <LuPencil size={14} />
        Rename
      </Menu.Item>
      <Menu.Item onClick={() => onMove(targets)} className={menuItem}>
        <LuFolderInput size={14} />
        Move to…
      </Menu.Item>
      <Menu.Separator className={menuSeparator} />
      <Menu.Item onClick={() => onDelete(targets)} className={dangerMenuItem}>
        <LuTrash2 size={14} />
        Delete
      </Menu.Item>
    </>
  );
}

// the ⋯ button at the end of a row
export function EntryRowMenu({ entry, ...actions }: EntryActions & { entry: Entry }) {
  return (
    <Menu.Root>
      <Menu.Trigger aria-label={`Actions for ${entry.name}`} className={iconButton}>
        <LuEllipsis size={15} />
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner align="end" sideOffset={4} className="z-50">
          <Menu.Popup className={`w-48 rounded-lg p-1 ${popup}`}>
            <EntryMenuItems targets={[entry]} {...actions} />
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}
