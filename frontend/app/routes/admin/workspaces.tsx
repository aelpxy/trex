import { WorkspaceList } from "~/components/admin/workspace-list";
import { listParams } from "~/components/admin/use-url-filter";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

import type { Route } from "./+types/workspaces";

export async function clientLoader({ request }: Route.ClientLoaderArgs) {
  const { page, search, sort } = listParams(request);
  await Promise.all([
    queryClient.ensureQueryData(queries.admin.workspaces(page, search, sort)),
    queryClient.ensureQueryData(queries.admin.models()),
    queryClient.ensureQueryData(queries.plans()),
  ]);
  return null;
}

export default function AdminWorkspaces() {
  return <WorkspaceList />;
}
