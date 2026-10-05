import { useId } from "react";
import { Popover } from "@base-ui/react/popover";
import { Radio } from "@base-ui/react/radio";
import { RadioGroup } from "@base-ui/react/radio-group";
import { Toggle } from "@base-ui/react/toggle";
import { ToggleGroup } from "@base-ui/react/toggle-group";
import { LuCheck, LuChevronDown, LuZap } from "react-icons/lu";

import { Switch } from "~/components/ui/switch";
import { focusRing, menuGroupLabel, menuSeparator, popup } from "~/components/ui/styles";

import { ContextDetails, type ContextUsage } from "./context-meter";
import { effortsFor, MODELS, supportsFast, type ChatSettings } from "./models";

const FAST_MULTIPLIER = 2;

// segments are narrow, so long level names get a short form; the full name is read out and described below
const SHORT_EFFORTS: Record<string, string> = { minimal: "Min", medium: "Med", xhigh: "X-High" };

type ModelPickerProps = { settings: ChatSettings; onChange: (patch: Partial<ChatSettings>) => void; context?: ContextUsage };

// everything about how the model answers in one panel by the send button, since it's set together and rarely
export function ModelPicker({ settings, onChange, context }: ModelPickerProps) {
  const model = MODELS.find((option) => option.value === settings.model);
  const efforts = effortsFor(settings.model);
  const effort = efforts.find((option) => option.value === settings.effort);
  const fastAvailable = supportsFast(settings.model);
  const fast = settings.fast && fastAvailable;
  const thinkingLabel = useId();

  return (
    <Popover.Root>
      <Popover.Trigger
        aria-label={`Model: ${model?.label ?? settings.model}, ${effort?.label ?? settings.effort} thinking${fast ? ", fast" : ""}`}
        className={`inline-flex h-8 min-w-0 cursor-pointer items-center gap-1 rounded-md px-2 text-xs font-medium text-muted transition-colors select-none hover:bg-subtle hover:text-ink data-popup-open:bg-subtle data-popup-open:text-ink ${focusRing}`}
      >
        {fast && <LuZap size={12} className="shrink-0 fill-current" />}
        <span className="truncate">{model?.label ?? settings.model}</span>
        <LuChevronDown size={12} className="shrink-0 opacity-60" />
      </Popover.Trigger>
      <Popover.Portal>
        <Popover.Positioner side="bottom" align="end" sideOffset={8} className="z-50">
          <Popover.Popup className={`max-h-(--available-height) w-80 max-w-(--available-width) overflow-y-auto rounded-lg p-1 ${popup}`}>
            <Popover.Title className={menuGroupLabel}>Model</Popover.Title>
            <RadioGroup aria-label="Model" value={settings.model} onValueChange={(value) => onChange({ model: value as string })}>
              {MODELS.map((option) => (
                <label
                  key={option.value}
                  title={`${option.description} context${supportsFast(option.value) ? ", fast mode" : ""}`}
                  className="flex h-8 cursor-pointer items-center gap-2 rounded-md px-2.5 text-[13px] select-none hover:bg-subtle has-focus-visible:bg-subtle"
                >
                  <Radio.Root value={option.value} className="sr-only" />
                  <LuCheck size={14} aria-hidden className={`shrink-0 text-ink ${option.value === settings.model ? "" : "invisible"}`} />
                  <span className="min-w-0 flex-1 truncate text-ink">{option.label}</span>
                  {supportsFast(option.value) && <LuZap size={12} aria-label="Fast mode available" className="shrink-0 text-muted" />}
                  <span className="shrink-0 text-xs text-muted tabular-nums">{option.description}</span>
                </label>
              ))}
            </RadioGroup>
            {efforts.length > 1 && (
              <>
                <div className={menuSeparator} />
                <div className="px-2.5 py-1.5">
                  <p id={thinkingLabel} className="mb-1.5 text-[11px] font-medium text-muted">
                    Thinking
                  </p>
                  <ToggleGroup
                    aria-labelledby={thinkingLabel}
                    value={[settings.effort]}
                    // pressing the chosen level again keeps it rather than leaving none
                    onValueChange={(value) => value[0] && onChange({ effort: value[0] as string })}
                    className="grid auto-cols-fr grid-flow-col gap-0.5 rounded-md bg-subtle p-0.5"
                  >
                    {efforts.map((option) => (
                      <Toggle
                        key={option.value}
                        value={option.value}
                        aria-label={option.label}
                        className={`h-7 min-w-0 cursor-pointer truncate rounded px-1.5 text-xs text-muted transition-colors hover:text-ink data-pressed:bg-surface data-pressed:font-medium data-pressed:text-ink data-pressed:shadow-sm ${focusRing}`}
                      >
                        {SHORT_EFFORTS[option.value] ?? option.label}
                      </Toggle>
                    ))}
                  </ToggleGroup>
                  <p className="mt-1.5 text-xs text-muted">{effort && `${effort.label}: ${effort.description}`}</p>
                </div>
              </>
            )}
            {fastAvailable && (
              <>
                <div className={menuSeparator} />
                <div className="px-2.5 py-2">
                  <Switch checked={settings.fast} onCheckedChange={(checked) => onChange({ fast: checked })} description={`Quicker replies, ${FAST_MULTIPLIER}× the usage`}>
                    Fast mode
                  </Switch>
                </div>
              </>
            )}
            {context && (
              <>
                <div className={menuSeparator} />
                <div className="px-1.5 py-2">
                  <ContextDetails context={context} />
                </div>
              </>
            )}
          </Popover.Popup>
        </Popover.Positioner>
      </Popover.Portal>
    </Popover.Root>
  );
}
