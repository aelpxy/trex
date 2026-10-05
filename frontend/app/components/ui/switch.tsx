import type { ReactNode } from "react";
import { Switch as BaseSwitch } from "@base-ui/react/switch";

import { focusRingOutset } from "./styles";

type SwitchProps = { checked: boolean; onCheckedChange: (checked: boolean) => void; disabled?: boolean; children: ReactNode; description?: ReactNode };

export function Switch({ checked, onCheckedChange, disabled, children, description }: SwitchProps) {
  return (
    <label className="flex cursor-pointer items-start gap-3 has-data-disabled:cursor-not-allowed has-data-disabled:opacity-60">
      <span className="min-w-0 flex-1">
        <span className="block text-[13px]">{children}</span>
        {description && <span className="mt-0.5 block text-[11px] text-muted">{description}</span>}
      </span>
      <BaseSwitch.Root
        checked={checked}
        onCheckedChange={onCheckedChange}
        disabled={disabled}
        className={`relative mt-0.5 flex h-5 w-9 shrink-0 rounded-full bg-line p-0.5 transition-colors data-checked:bg-ink ${focusRingOutset}`}
      >
        <BaseSwitch.Thumb className="size-4 rounded-full bg-surface shadow-sm transition-transform data-checked:translate-x-4" />
      </BaseSwitch.Root>
    </label>
  );
}
