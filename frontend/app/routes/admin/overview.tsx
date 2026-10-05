import { Overview } from "~/components/admin/overview";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

export async function clientLoader() {
  await queryClient.ensureQueryData(queries.admin.overview());
  return null;
}

export default function AdminOverview() {
  return <Overview />;
}
