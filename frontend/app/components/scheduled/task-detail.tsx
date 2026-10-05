import { useSuspenseQuery } from "@tanstack/react-query";
import { Link, useNavigate } from "react-router";
import { LuArrowLeft, LuCircleAlert } from "react-icons/lu";

import { focusRing } from "~/components/ui/styles";
import { queries } from "~/lib/queries";
import { UNTITLED } from "~/lib/workspace";

import { describeSchedule, formatRunTime } from "./schedule";
import { TaskActions } from "./task-actions";

const STATUS_LABEL = { idle: "Done", running: "Running", needs_input: "Waiting", failed: "Failed" };

export function TaskDetail({ id }: { id: string }) {
  const navigate = useNavigate();
  const { data: task } = useSuspenseQuery(queries.scheduled.task(id));
  const { data: runs } = useSuspenseQuery(queries.scheduled.runs(id));

  return (
    <div className="mt-6 space-y-8">
      <Link to="/scheduled" className={`inline-flex items-center gap-1.5 rounded-md text-xs text-muted hover:text-ink ${focusRing}`}>
        <LuArrowLeft size={13} />
        All scheduled tasks
      </Link>
      <section className="ui-card px-5 py-4">
        <div className="flex items-start gap-3">
          <div className="min-w-0 flex-1">
            <h2 className="flex items-center gap-2 text-base font-medium">
              {task.title}
              {task.paused && <span className="rounded bg-subtle px-1.5 py-0.5 text-[10px] font-medium text-muted">Paused</span>}
            </h2>
            <p className="mt-1 text-xs text-muted">
              {describeSchedule(task.schedule)}, {task.timezone.replaceAll("_", " ")} time · {task.model}
            </p>
            <p className="mt-1 text-xs text-muted">
              {task.paused ? "Paused" : task.next_run_at ? `Next run ${formatRunTime(task.next_run_at)}` : "Won't run again"}
              {task.last_run_at && ` · last run ${formatRunTime(task.last_run_at)}`}
            </p>
          </div>
          <TaskActions task={task} onDeleted={() => navigate("/scheduled", { replace: true })} />
        </div>
        <p className="mt-4 text-sm leading-6 whitespace-pre-wrap">{task.prompt}</p>
        {task.last_error && (
          <p role="alert" className="mt-3 flex items-center gap-1.5 text-xs text-danger">
            <LuCircleAlert size={13} className="shrink-0" />
            The last run didn't start: {task.last_error}
          </p>
        )}
      </section>
      <section>
        <h2 className="text-sm font-medium">Runs</h2>
        {runs.length === 0 ? (
          <p className="mt-2 text-sm text-muted">No runs yet. Use "Run now" from the menu to try it.</p>
        ) : (
          <ul className="ui-card mt-3 divide-y divide-line overflow-hidden">
            {runs.map((run) => (
              <li key={run.id}>
                <Link to={`/chat/${run.id}`} className={`flex items-center gap-3 px-4 py-2.5 text-sm hover:bg-subtle/60 ${focusRing}`}>
                  <span className="min-w-0 flex-1 truncate">{run.title ?? UNTITLED}</span>
                  <span className={`shrink-0 text-xs ${run.status === "failed" ? "text-danger" : "text-muted"}`}>{STATUS_LABEL[run.status]}</span>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
