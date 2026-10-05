import { redirect } from "react-router";

import { TaskDetail } from "~/components/scheduled/task-detail";
import { Page } from "~/components/ui/page";
import { ApiError } from "~/lib/api";
import { pageTitle } from "~/lib/meta";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

import type { Route } from "./+types/scheduled-task";

export async function clientLoader({ params }: Route.ClientLoaderArgs) {
  try {
    const [task] = await Promise.all([
      queryClient.ensureQueryData(queries.scheduled.task(params.taskId)),
      queryClient.ensureQueryData(queries.scheduled.runs(params.taskId)),
    ]);
    return { title: task.title };
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) throw redirect("/scheduled");
    throw error;
  }
}

export const meta = ({ loaderData }: Route.MetaArgs) => pageTitle(loaderData?.title ?? "Scheduled");

export default function ScheduledTask({ params }: Route.ComponentProps) {
  return (
    <Page title="Scheduled">
      <TaskDetail id={params.taskId} />
    </Page>
  );
}
