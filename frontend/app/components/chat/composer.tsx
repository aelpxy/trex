import { useLayoutEffect, useRef, useState, type FormEvent, type KeyboardEvent } from "react";
import { Button } from "@base-ui/react/button";
import { LuArrowUp, LuBrain, LuSquare } from "react-icons/lu";

import { focusRingOutset } from "~/components/ui/styles";

import { FastToggle } from "./fast-toggle";
import { effortsFor, MODELS, type ChatSettings } from "./models";
import { OptionSelect } from "./option-select";

const MAX_HEIGHT_PX = 200;

type ComposerProps = {
  streaming: boolean;
  settings: ChatSettings;
  onSettingsChange: (patch: Partial<ChatSettings>) => void;
  onSend: (content: string) => void;
  onStop: () => void;
};

export function Composer({ streaming, settings, onSettingsChange, onSend, onStop }: ComposerProps) {
  const [value, setValue] = useState("");
  const textarea = useRef<HTMLTextAreaElement>(null);
  const canSend = value.trim().length > 0 && !streaming;

  useLayoutEffect(() => {
    const element = textarea.current;
    if (!element) return;
    element.style.height = "auto";
    element.style.height = `${Math.min(element.scrollHeight, MAX_HEIGHT_PX)}px`;
  }, [value]);

  function submit(event?: FormEvent) {
    event?.preventDefault();
    if (!canSend) return;
    onSend(value.trim());
    setValue("");
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) submit(event);
  }

  return (
    <form onSubmit={submit} className="glass rounded-2xl border border-line p-2 shadow-sm transition-colors focus-within:border-muted/50">
      <textarea
        ref={textarea}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={onKeyDown}
        rows={1}
        autoFocus
        aria-label="Message"
        placeholder="Ask anything"
        className="block max-h-50 w-full resize-none bg-transparent px-2 py-1.5 text-sm leading-6 text-ink outline-none placeholder:text-muted"
      />
      <div className="flex items-center gap-1">
        <div className="flex min-w-0 flex-1 items-center gap-0.5">
          <OptionSelect label="Model" options={MODELS} value={settings.model} onChange={(model) => onSettingsChange({ model })} />
          <OptionSelect label="Thinking effort" options={effortsFor(settings.model)} value={settings.effort} onChange={(effort) => onSettingsChange({ effort })} icon={<LuBrain size={13} className="shrink-0" />} />
          <FastToggle pressed={settings.fast} onChange={(fast) => onSettingsChange({ fast })} />
        </div>
        {streaming ? (
          <Button type="button" onClick={onStop} aria-label="Stop" className={`inline-flex size-8 cursor-pointer items-center justify-center rounded-full bg-ink text-on-solid transition-colors hover:bg-ink/85 ${focusRingOutset}`}>
            <LuSquare size={12} fill="currentColor" />
          </Button>
        ) : (
          <Button type="submit" disabled={!canSend} focusableWhenDisabled aria-label="Send" className={`inline-flex size-8 cursor-pointer items-center justify-center rounded-full bg-ink text-on-solid transition-colors hover:bg-ink/85 data-disabled:cursor-not-allowed data-disabled:opacity-30 ${focusRingOutset}`}>
            <LuArrowUp size={16} />
          </Button>
        )}
      </div>
    </form>
  );
}
