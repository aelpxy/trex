import type { ReactNode } from "react";
import { ContextMenu } from "@base-ui/react/context-menu";
import { LuTrash2 } from "react-icons/lu";

import { dangerMenuItem, popup } from "~/components/ui/styles";
import type { DeleteTarget } from "~/components/ui/delete-confirm-dialog";
import { useWorkspace } from "~/components/workspace/workspace-provider";

type ItemContextMenuProps = { target: DeleteTarget; children: ReactNode };

export function ItemContextMenu({ target, children }: ItemContextMenuProps) {
  const { requestDelete } = useWorkspace();

  return (
    <ContextMenu.Root>
      <ContextMenu.Trigger className="block">{children}</ContextMenu.Trigger>
      <ContextMenu.Portal>
        <ContextMenu.Positioner className="z-50">
          <ContextMenu.Popup className={`w-48 rounded-lg p-1 ${popup}`}>
            <ContextMenu.Item onClick={() => requestDelete(target)} className={dangerMenuItem}>
              <LuTrash2 size={14} />
              Delete {target.kind}
            </ContextMenu.Item>
          </ContextMenu.Popup>
        </ContextMenu.Positioner>
      </ContextMenu.Portal>
    </ContextMenu.Root>
  );
}
