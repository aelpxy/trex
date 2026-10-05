import type { ReactNode } from "react";
import { Checkbox as BaseCheckbox } from "@base-ui/react/checkbox";
import { LuCheck, LuMinus } from "react-icons/lu";

import { focusRingOutset } from "./styles";

type CheckboxProps = {
  checked: boolean;
  onCheckedChange: (checked: boolean) => void;
  // without visible text, `label` names it for screen readers
  children?: ReactNode;
  label?: string;
  indeterminate?: boolean;
  disabled?: boolean;
};

export function Checkbox({ checked, onCheckedChange, children, label, indeterminate, disabled }: CheckboxProps) {
  return (
    <label className="flex cursor-pointer items-center gap-2 text-[13px] has-data-disabled:cursor-not-allowed has-data-disabled:opacity-60">
      <BaseCheckbox.Root
        checked={checked}
        indeterminate={indeterminate}
        onCheckedChange={onCheckedChange}
        disabled={disabled}
        aria-label={label}
        className={`flex size-4 shrink-0 items-center justify-center rounded border border-line bg-surface transition-colors data-checked:border-ink data-checked:bg-ink data-indeterminate:border-ink data-indeterminate:bg-ink ${focusRingOutset}`}
      >
        <BaseCheckbox.Indicator className="text-on-solid">{indeterminate ? <LuMinus size={12} strokeWidth={3} /> : <LuCheck size={12} strokeWidth={3} />}</BaseCheckbox.Indicator>
      </BaseCheckbox.Root>
      {children && <span className="min-w-0 truncate">{children}</span>}
    </label>
  );
}
