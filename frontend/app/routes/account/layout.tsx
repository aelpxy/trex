import { Suspense } from "react";
import { Outlet } from "react-router";

import { Page } from "~/components/ui/page";
import { Skeleton } from "~/components/ui/skeleton";
import { TabNav } from "~/components/ui/tab-nav";
import { pageTitle } from "~/lib/meta";

export const meta = () => pageTitle("Account");

const TABS = [
  { to: "/account", label: "Billing", end: true },
  { to: "/account/profile", label: "Profile" },
  { to: "/account/devices", label: "Devices" },
];

export default function AccountLayout() {
  return (
    <Page title="Account">
      <TabNav tabs={TABS} label="Account" />
      <div className="mt-6">
        <Suspense fallback={<Skeleton className="h-40 w-full" />}>
          <Outlet />
        </Suspense>
      </div>
    </Page>
  );
}
