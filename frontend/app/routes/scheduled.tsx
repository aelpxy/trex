import { LuCalendarClock, LuPlus } from "react-icons/lu";

import { Button } from "~/components/ui/button";
import { EmptyState } from "~/components/ui/empty-state";
import { Page } from "~/components/ui/page";
import { pageTitle } from "~/lib/meta";

export const meta = () => pageTitle("Scheduled");

export default function Scheduled() {
  return (
    <Page title="Scheduled">
      <EmptyState
        icon={LuCalendarClock}
        title="No scheduled tasks"
        description="Run a prompt on a schedule, like a daily report or a weekly dependency check."
        action={
          <Button>
            <LuPlus size={14} />
            New schedule
          </Button>
        }
      />
    </Page>
  );
}
