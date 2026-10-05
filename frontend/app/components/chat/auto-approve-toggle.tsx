import { Toggle } from "@base-ui/react/toggle";
import { Tooltip } from "@base-ui/react/tooltip";
import { LuShieldCheck } from "react-icons/lu";

import { focusRing, tooltip } from "~/components/ui/styles";

type AutoApproveToggleProps = { pressed: boolean; onChange: (pressed: boolean) => void };

// approves the sandbox's network access requests without asking, for tasks the user trusts
export function AutoApproveToggle({ pressed, onChange }: AutoApproveToggleProps) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        render={<Toggle pressed={pressed} onPressedChange={onChange} />}
        aria-description="Approves network access requests without asking"
        className={`group inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-xs font-medium text-muted transition-colors select-none hover:bg-subtle hover:text-ink data-pressed:bg-subtle data-pressed:text-ink ${focusRing}`}
      >
        <LuShieldCheck size={13} />
        <span className="pointer-coarse:hidden">Auto-approve</span>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Positioner side="top" sideOffset={8}>
          <Tooltip.Popup className={`max-w-60 ${tooltip}`}>
            <span className="font-medium">Auto-approve{pressed ? " is on" : ""}</span>
            <span className="block text-muted">Network access the agent needs is approved without asking, and still listed in the chat. Turn it on only for tasks you trust.</span>
          </Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}
