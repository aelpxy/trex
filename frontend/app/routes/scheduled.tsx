import { useState } from "react";
import { useSuspenseQuery } from "@tanstack/react-query";
import { LuCalendarClock, LuPlus } from "react-icons/lu";

import { TaskDialog } from "~/components/scheduled/task-dialog";
import { TaskList } from "~/components/scheduled/task-list";
import { Button } from "~/components/ui/button";
import { EmptyState } from "~/components/ui/empty-state";
import { Page } from "~/components/ui/page";
import { pageTitle } from "~/lib/meta";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

export const meta = () => pageTitle("Scheduled");

export async function clientLoader() {
  await queryClient.ensureQueryData(queries.scheduled.list());
  return null;
}

export default function Scheduled() {
  const { data: tasks } = useSuspenseQuery(queries.scheduled.list());
  const [creating, setCreating] = useState(false);
  const newTask = (
    <Button onClick={() => setCreating(true)}>
      <LuPlus size={14} />
      New schedule
    </Button>
  );

  return (
    <Page title="Scheduled">
      {tasks.length === 0 ? (
        <EmptyState icon={LuCalendarClock} title="No scheduled tasks" description="Run a prompt on a schedule, like a daily report or a weekly dependency check." action={newTask} />
      ) : (
        <>
          <div className="mt-6 flex justify-end">{newTask}</div>
          <TaskList />
        </>
      )}
      {creating && <TaskDialog open onOpenChange={setCreating} />}
    </Page>
  );
}
