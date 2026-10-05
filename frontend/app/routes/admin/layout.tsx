import { Suspense } from "react";
import { NavLink, Outlet, redirect } from "react-router";

import { Page } from "~/components/ui/page";
import { Skeleton } from "~/components/ui/skeleton";
import { focusRing } from "~/components/ui/styles";
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
      <nav aria-label="Admin" className="mt-6 flex gap-1 overflow-x-auto border-b border-line pb-2">
        {TABS.map((tab) => (
          <NavLink
            key={tab.to}
            to={tab.to}
            end={tab.end}
            className={`h-7 shrink-0 rounded-md px-2.5 text-xs leading-7 font-medium text-muted transition-colors hover:text-ink aria-[current=page]:bg-subtle aria-[current=page]:text-ink ${focusRing}`}
          >
            {tab.label}
          </NavLink>
        ))}
      </nav>
      <div className="mt-6">
        <Suspense fallback={<Skeleton className="h-40 w-full" />}>
          <Outlet />
        </Suspense>
      </div>
    </Page>
  );
}
