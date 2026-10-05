import { WorkspaceLibrary } from "~/components/admin/workspace-library";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";
import { listParams } from "~/lib/use-url-filter";

import type { Route } from "./+types/library";

// without a workspace it's a list to pick one from; with one, that workspace's files
export async function clientLoader({ request }: Route.ClientLoaderArgs) {
  const workspace = new URL(request.url).searchParams.get("workspace");
  if (workspace) {
    await Promise.all([queryClient.ensureQueryData(queries.admin.workspaces(1, workspace)), queryClient.ensureQueryData(queries.admin.library(workspace))]);
    return null;
  }
  const { page, search, sort } = listParams(request);
  await queryClient.ensureQueryData(queries.admin.workspaces(page, search, sort));
  return null;
}

export default function AdminLibrary() {
  return <WorkspaceLibrary />;
}
