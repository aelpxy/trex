import type { ReactNode } from "react";
import { Select } from "@base-ui/react/select";
import { LuCheck, LuChevronDown } from "react-icons/lu";

import { focusRing, popup } from "~/components/ui/styles";

import type { Option } from "./models";

type OptionSelectProps<T extends string> = {
  label: string;
  options: Option<T>[];
  value: T;
  onChange: (value: T) => void;
  icon?: ReactNode;
};

export function OptionSelect<T extends string>({ label, options, value, onChange, icon }: OptionSelectProps<T>) {
  return (
    <Select.Root items={options} value={value} onValueChange={(next) => next && onChange(next as T)}>
      <Select.Trigger
        aria-label={label}
        className={`inline-flex h-8 min-w-0 cursor-pointer items-center gap-1.5 rounded-md px-2.5 text-xs font-medium text-muted transition-colors select-none hover:bg-subtle hover:text-ink ${focusRing} data-popup-open:bg-subtle data-popup-open:text-ink`}
      >
        {icon}
        <Select.Value className="truncate" />
        <Select.Icon className="shrink-0 opacity-60">
          <LuChevronDown size={12} />
        </Select.Icon>
      </Select.Trigger>
      <Select.Portal>
        <Select.Positioner side="top" align="start" sideOffset={6} alignItemWithTrigger={false} className="z-50 outline-none select-none">
          <Select.Popup className={`w-64 rounded-lg p-1 ${popup}`}>
            <Select.List className="max-h-(--available-height) overflow-y-auto">
              {options.map((option) => (
                <Select.Item
                  key={option.value}
                  value={option.value}
                  className="grid cursor-pointer grid-cols-[1fr_1rem] items-center gap-3 rounded-md px-2.5 py-2 outline-none select-none data-highlighted:bg-subtle"
                >
                  <span className="flex min-w-0 items-start gap-2.5">
                    {option.icon && <option.icon size={14} className="mt-0.5 shrink-0 text-muted" />}
                    <span className="min-w-0">
                      <Select.ItemText className="block text-[13px] font-medium text-ink">{option.label}</Select.ItemText>
                      <span className="block text-xs text-muted">{option.description}</span>
                    </span>
                  </span>
                  <Select.ItemIndicator className="text-ink">
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
