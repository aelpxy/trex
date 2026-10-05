import { WorkspaceList } from "~/components/admin/workspace-list";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

export async function clientLoader() {
  await Promise.all([
    queryClient.ensureQueryData(queries.admin.workspaces()),
    queryClient.ensureQueryData(queries.admin.models()),
    queryClient.ensureQueryData(queries.plans()),
  ]);
  return null;
}

export default function AdminWorkspaces() {
  return <WorkspaceList />;
}
