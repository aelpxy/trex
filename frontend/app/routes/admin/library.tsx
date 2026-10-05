import { WorkspaceLibrary } from "~/components/admin/workspace-library";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

import type { Route } from "./+types/library";

export async function clientLoader({ request }: Route.ClientLoaderArgs) {
  const workspaces = await queryClient.ensureQueryData(queries.admin.workspaces());
  const requested = new URL(request.url).searchParams.get("workspace");
  const workspace = workspaces.find((candidate) => candidate.id === requested) ?? workspaces[0];
  if (workspace) await queryClient.ensureQueryData(queries.admin.library(workspace.id));
  return null;
}

export default function AdminLibrary() {
  return <WorkspaceLibrary />;
}
