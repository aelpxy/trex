import { Toggle } from "@base-ui/react/toggle";
import { Tooltip } from "@base-ui/react/tooltip";
import { LuZap } from "react-icons/lu";

import { focusRing, tooltip } from "~/components/ui/styles";

const USAGE_MULTIPLIER = 2;

type FastToggleProps = { pressed: boolean; onChange: (pressed: boolean) => void };

export function FastToggle({ pressed, onChange }: FastToggleProps) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        render={<Toggle pressed={pressed} onPressedChange={onChange} />}
        aria-description={`Uses ${USAGE_MULTIPLIER}× more usage`}
        className={`group inline-flex h-8 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-xs font-medium text-muted transition-colors select-none hover:bg-subtle hover:text-ink data-pressed:bg-subtle data-pressed:text-ink ${focusRing}`}
      >
        <LuZap size={13} className="group-data-pressed:fill-current" />
        Fast
        {pressed && <span className="rounded bg-ink px-1 font-mono text-[10px] leading-4 text-on-solid">{USAGE_MULTIPLIER}×</span>}
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Positioner side="top" sideOffset={8}>
          <Tooltip.Popup className={`max-w-56 ${tooltip}`}>
            <span className="font-medium">Fast mode{pressed ? " is on" : ""}</span>
            <span className="block text-muted">Priority processing for quicker replies. Uses {USAGE_MULTIPLIER}× more usage.</span>
          </Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}
