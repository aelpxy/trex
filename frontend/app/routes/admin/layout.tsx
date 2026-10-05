import { Suspense } from "react";
import { Outlet, redirect } from "react-router";

import { Page } from "~/components/ui/page";
import { Skeleton } from "~/components/ui/skeleton";
import { TabNav } from "~/components/ui/tab-nav";
import { pageTitle } from "~/lib/meta";
import { trex } from "~/lib/trex";

export const meta = () => pageTitle("Admin");

// everyone but admins is sent home; the api refuses them anyway
export async function clientLoader() {
  const me = await trex.me();
  if (me.user.role !== "admin") throw redirect("/");
  return null;
}

const TABS = [
  { to: "/admin", label: "Overview", end: true },
  { to: "/admin/users", label: "Users" },
  { to: "/admin/workspaces", label: "Workspaces" },
  { to: "/admin/library", label: "Library" },
  { to: "/admin/usage", label: "Usage" },
  { to: "/admin/logs", label: "Logs" },
];

export default function AdminLayout() {
  return (
    <Page title="Admin">
      <TabNav tabs={TABS} label="Admin" />
      <div className="mt-6">
        <Suspense fallback={<Skeleton className="h-40 w-full" />}>
          <Outlet />
        </Suspense>
      </div>
    </Page>
  );
}
