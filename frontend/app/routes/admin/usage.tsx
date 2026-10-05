import { useSearchParams } from "react-router";

import { UsageReport, usageDays } from "~/components/admin/usage-report";
import { queries } from "~/lib/queries";
import { queryClient } from "~/lib/query-client";

import type { Route } from "./+types/usage";

export async function clientLoader({ request }: Route.ClientLoaderArgs) {
  await queryClient.ensureQueryData(queries.admin.usage(usageDays(new URL(request.url).searchParams)));
  return null;
}

export default function AdminUsage() {
  const [params] = useSearchParams();
  return <UsageReport days={usageDays(params)} />;
}
