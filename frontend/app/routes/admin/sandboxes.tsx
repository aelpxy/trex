import { SandboxList } from "~/components/admin/sandbox-list";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

export async function clientLoader() {
  await queryClient.ensureQueryData(queries.admin.sandboxes());
  return null;
}

export default function AdminSandboxes() {
  return <SandboxList />;
}
