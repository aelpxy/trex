import { Select } from "@base-ui/react/select";
import { LuCheck, LuChevronDown } from "react-icons/lu";

import { focusRing, popup } from "./styles";

export type SelectOption = { value: string; label: string };

type SelectFieldProps = { label: string; options: SelectOption[]; value: string; onChange: (value: string) => void };

// a form select that looks like the other inputs
export function SelectField({ label, options, value, onChange }: SelectFieldProps) {
  return (
    <Select.Root items={options} value={value} onValueChange={(next) => next !== null && onChange(next)}>
      <Select.Trigger aria-label={label} className={`ui-input flex h-10 cursor-pointer items-center justify-between gap-2 text-left ${focusRing} data-popup-open:border-muted/40`}>
        <Select.Value className="truncate" />
        <Select.Icon className="shrink-0 text-muted">
          <LuChevronDown size={14} />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal>
        <Select.Positioner sideOffset={6} alignItemWithTrigger={false} className="z-50 outline-none select-none">
          <Select.Popup className={`w-(--anchor-width) rounded-lg p-1 ${popup}`}>
            <Select.List className="max-h-(--available-height) overflow-y-auto">
              {options.map((option) => (
                <Select.Item key={option.value} value={option.value} className="flex h-8 cursor-pointer items-center gap-2 rounded-md px-2.5 text-[13px] text-muted outline-none select-none data-highlighted:bg-subtle data-highlighted:text-ink data-selected:text-ink">
                  <Select.ItemText className="min-w-0 flex-1 truncate">{option.label}</Select.ItemText>
                  <Select.ItemIndicator className="shrink-0">
                    <LuCheck size={14} />
                  </Select.ItemIndicator>
                </Select.Item>
              ))}
            </Select.List>
          </Select.Popup>
        </Select.Positioner>
      </Select.Portal>
    </Select.Root>
  );
}
