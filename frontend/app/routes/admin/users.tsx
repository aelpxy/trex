import { UserList } from "~/components/admin/user-list";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";
import { listParams } from "~/lib/use-url-filter";

import type { Route } from "./+types/users";

export async function clientLoader({ request }: Route.ClientLoaderArgs) {
  const { page, search, sort } = listParams(request);
  await Promise.all([
    queryClient.ensureQueryData(queries.admin.users(page, search, sort)),
    queryClient.ensureQueryData(queries.admin.models()),
    queryClient.ensureQueryData(queries.plans()),
  ]);
  return null;
}

export default function AdminUsers() {
  return <UserList />;
}
