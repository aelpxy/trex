import { useSuspenseQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { LuCircleAlert } from "react-icons/lu";

import { focusRing } from "~/components/ui/styles";
import { queries } from "~/lib/queries";

import { describeSchedule, formatRunTime } from "./schedule";
import { TaskActions } from "./task-actions";

export function TaskList() {
  const { data: tasks } = useSuspenseQuery(queries.scheduled.list());
  return (
    <ul className="ui-card mt-6 divide-y divide-line overflow-hidden">
      {tasks.map((task) => (
        <li key={task.id} className="flex items-center gap-3 px-4 py-3">
          <Link to={`/scheduled/${task.id}`} className={`min-w-0 flex-1 rounded-md ${focusRing}`}>
            <span className="flex items-center gap-2 text-sm font-medium">
              <span className="truncate">{task.title}</span>
              {task.paused && <span className="rounded bg-subtle px-1.5 py-0.5 text-[10px] font-medium text-muted">Paused</span>}
            </span>
            <span className="block truncate text-xs text-muted">
              {describeSchedule(task.schedule)}
              {!task.paused && task.next_run_at && ` · next ${formatRunTime(task.next_run_at)}`}
            </span>
            {task.last_error && (
              <span className="mt-0.5 flex items-center gap-1 truncate text-xs text-danger">
                <LuCircleAlert size={12} className="shrink-0" />
                Last run didn't start: {task.last_error}
              </span>
            )}
          </Link>
          <TaskActions task={task} />
        </li>
      ))}
    </ul>
  );
}
