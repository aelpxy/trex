import { RunList } from "~/components/admin/run-list";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

export async function clientLoader() {
  await queryClient.ensureQueryData(queries.admin.runs());
  return null;
}

export default function AdminRuns() {
  return <RunList />;
}
