import { useState } from "react";
import { Menu } from "@base-ui/react/menu";
import { useNavigate } from "react-router";
import { LuEllipsis, LuPause, LuPencil, LuPlay, LuTrash2, LuZap } from "react-icons/lu";

import { DeleteConfirmDialog, type DeleteTarget } from "~/components/ui/delete-confirm-dialog";
import { dangerMenuItem, iconButton, menuItem, menuSeparator, popup } from "~/components/ui/styles";
import type { ApiScheduledTask } from "~/lib/trex";

import { useDeleteTask, usePauseTask, useRunTask } from "./mutations";
import { TaskDialog } from "./task-dialog";

// run now, pause or resume, edit and delete, from a task's menu
export function TaskActions({ task, onDeleted }: { task: ApiScheduledTask; onDeleted?: () => void }) {
  const navigate = useNavigate();
  const [editing, setEditing] = useState(false);
  const [deleting, setDeleting] = useState<DeleteTarget | null>(null);
  const run = useRunTask();
  const pause = usePauseTask();
  const remove = useDeleteTask();
  const error = run.error ?? pause.error ?? remove.error;

  return (
    <div className="flex items-center gap-2">
      {error && <span role="alert" className="max-w-56 truncate text-xs text-danger" title={error.message}>{error.message}</span>}
      <Menu.Root>
        <Menu.Trigger aria-label={`Actions for ${task.title}`} className={iconButton}>
          <LuEllipsis size={15} />
        </Menu.Trigger>
        <Menu.Portal>
          <Menu.Positioner align="end" sideOffset={4} className="z-50">
            <Menu.Popup className={`w-48 rounded-lg p-1 ${popup}`}>
              <Menu.Item onClick={() => run.mutate(task.id, { onSuccess: (session) => navigate(`/chat/${session.id}`) })} className={menuItem}>
                <LuZap size={14} />
                Run now
              </Menu.Item>
              <Menu.Item onClick={() => pause.mutate({ id: task.id, paused: !task.paused })} className={menuItem}>
                {task.paused ? <LuPlay size={14} /> : <LuPause size={14} />}
                {task.paused ? "Resume" : "Pause"}
              </Menu.Item>
              <Menu.Item onClick={() => setEditing(true)} className={menuItem}>
                <LuPencil size={14} />
                Edit
              </Menu.Item>
              <Menu.Separator className={menuSeparator} />
              <Menu.Item onClick={() => setDeleting({ kind: "task", id: task.id, name: task.title })} className={dangerMenuItem}>
                <LuTrash2 size={14} />
                Delete
              </Menu.Item>
            </Menu.Popup>
          </Menu.Positioner>
        </Menu.Portal>
      </Menu.Root>
      {editing && <TaskDialog open onOpenChange={setEditing} task={task} />}
      <DeleteConfirmDialog
        target={deleting}
        onConfirm={(target) => {
          setDeleting(null);
          remove.mutate(target.id, { onSuccess: onDeleted });
        }}
        onCancel={() => setDeleting(null)}
      />
    </div>
  );
}
