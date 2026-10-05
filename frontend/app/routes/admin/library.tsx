import { WorkspaceLibrary } from "~/components/admin/workspace-library";
import { listParams } from "~/components/admin/use-url-filter";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

import type { Route } from "./+types/library";

export async function clientLoader({ request }: Route.ClientLoaderArgs) {
  const { search } = listParams(request);
  const requested = new URL(request.url).searchParams.get("workspace") ?? "";
  const [matching, browsed] = await Promise.all([
    queryClient.ensureQueryData(queries.admin.workspaces(1, search)),
    queryClient.ensureQueryData(queries.admin.workspaces(1, requested)),
  ]);
  const workspace = browsed.data.find((candidate) => candidate.id === requested) ?? matching.data[0];
  if (workspace) await queryClient.ensureQueryData(queries.admin.library(workspace.id));
  return null;
}

export default function AdminLibrary() {
  return <WorkspaceLibrary />;
}
