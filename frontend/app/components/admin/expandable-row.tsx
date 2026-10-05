import { Collapsible } from "@base-ui/react/collapsible";
import type { ReactNode } from "react";
import { LuChevronRight } from "react-icons/lu";

import { focusRing } from "~/components/ui/styles";

// a list row that opens to show its controls
export function ExpandableRow({ summary, children }: { summary: ReactNode; children: ReactNode }) {
  return (
    <Collapsible.Root render={<li />} className="border-b border-line last:border-0">
      <Collapsible.Trigger className={`group flex w-full cursor-pointer items-center gap-4 px-4 py-3 text-left text-sm hover:bg-subtle/60 ${focusRing}`}>
        {summary}
        <LuChevronRight size={13} className="shrink-0 text-muted transition-transform duration-150 group-data-panel-open:rotate-90" />
      </Collapsible.Trigger>
      <Collapsible.Panel>{children}</Collapsible.Panel>
    </Collapsible.Root>
  );
}
