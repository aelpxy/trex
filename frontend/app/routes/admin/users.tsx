import { UserList } from "~/components/admin/user-list";
import { listParams } from "~/components/admin/use-url-filter";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

import type { Route } from "./+types/users";

export async function clientLoader({ request }: Route.ClientLoaderArgs) {
  const { page, search } = listParams(request);
  await Promise.all([
    queryClient.ensureQueryData(queries.admin.users(page, search)),
    queryClient.ensureQueryData(queries.admin.models()),
    queryClient.ensureQueryData(queries.plans()),
  ]);
  return null;
}

export default function AdminUsers() {
  return <UserList />;
}
