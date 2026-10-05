import { useEffect, useLayoutEffect, useRef, useState, type ClipboardEvent, type FormEvent, type KeyboardEvent } from "react";
import { Button } from "@base-ui/react/button";
import { LuArrowUp, LuBrain, LuClock, LuFileText, LuPaperclip, LuSquare, LuX } from "react-icons/lu";

import { focusRingOutset, iconButton } from "~/components/ui/styles";
import { ATTACHMENT_TYPES, kindOf, MAX_ATTACHMENT_BYTES, MAX_ATTACHMENTS, MAX_MESSAGE_CHARS, readAsDataUrl } from "~/lib/attachments";

import { FastToggle } from "./fast-toggle";
import { effortsFor, MODELS, supportsFast, type ChatSettings } from "./models";
import { OptionSelect } from "./option-select";
import type { OutgoingAttachment, QueuedMessage } from "./use-chat";

const MAX_HEIGHT_PX = 200;
const MAX_ATTACHMENT_MB = MAX_ATTACHMENT_BYTES / 1024 / 1024;

type ComposerProps = {
  streaming: boolean;
  settings: ChatSettings;
  queued?: QueuedMessage[];
  onSettingsChange: (patch: Partial<ChatSettings>) => void;
  onSend: (content: string, attachments: OutgoingAttachment[], interrupt: boolean) => void;
  onStop: () => void;
  // files dropped on the chat, attached once each
  dropped?: { files: File[]; id: number };
};

const isMac = () => typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);

const roundButton = `inline-flex size-8 cursor-pointer items-center justify-center rounded-full bg-ink text-on-solid transition-colors hover:bg-ink/85 data-disabled:cursor-not-allowed data-disabled:opacity-30 ${focusRingOutset}`;

function AttachmentChip({ attachment, onRemove }: { attachment: OutgoingAttachment; onRemove: () => void }) {
  return (
    <div className="group relative">
      {attachment.kind === "image" ? (
        <img src={attachment.url} alt={attachment.name} className="size-14 rounded-lg border border-line object-cover" />
      ) : (
        <div className="flex h-14 max-w-48 items-center gap-2 rounded-lg border border-line bg-subtle/60 px-3">
          <LuFileText size={16} className="shrink-0 text-muted" />
          <span className="truncate text-xs">{attachment.name}</span>
        </div>
      )}
      <Button
        type="button"
        onClick={onRemove}
        aria-label={`Remove ${attachment.name}`}
        className={`absolute -top-1.5 -right-1.5 inline-flex size-5 cursor-pointer items-center justify-center rounded-full border border-line bg-surface text-muted hover:text-ink ${focusRingOutset}`}
      >
        <LuX size={11} />
      </Button>
    </div>
  );
}

export function Composer({ streaming, settings, queued = [], dropped, onSettingsChange, onSend, onStop }: ComposerProps) {
  const [value, setValue] = useState("");
  const [attachments, setAttachments] = useState<OutgoingAttachment[]>([]);
  const [error, setError] = useState<string | null>(null);
  const textarea = useRef<HTMLTextAreaElement>(null);
  const fileInput = useRef<HTMLInputElement>(null);
  const canSend = value.trim().length > 0 || attachments.length > 0;

  useLayoutEffect(() => {
    const element = textarea.current;
    if (!element) return;
    element.style.height = "auto";
    element.style.height = `${Math.min(element.scrollHeight, MAX_HEIGHT_PX)}px`;
  }, [value]);

  const handledDrop = useRef<number | undefined>(undefined);
  useEffect(() => {
    if (!dropped || handledDrop.current === dropped.id) return;
    handledDrop.current = dropped.id;
    void attach(dropped.files);
  }, [dropped]);

  async function attach(files: File[]) {
    setError(null);
    const room = MAX_ATTACHMENTS - attachments.length;
    if (files.length > room) setError(`A message can have up to ${MAX_ATTACHMENTS} files.`);
    const tooBig = files.find((file) => file.size > MAX_ATTACHMENT_BYTES);
    if (tooBig) setError(`${tooBig.name} is larger than ${MAX_ATTACHMENT_MB} MB.`);
    const accepted = files.filter((file) => file.size <= MAX_ATTACHMENT_BYTES).slice(0, Math.max(room, 0));
    try {
      const read = await Promise.all(accepted.map(async (file) => ({ name: file.name, kind: kindOf(file.type), url: await readAsDataUrl(file) })));
      const total = [...attachments, ...read].reduce((sum, file) => sum + file.url.length, 0);
      if (total > MAX_MESSAGE_CHARS) return setError("These files are too large to send together.");
      setAttachments((current) => [...current, ...read].slice(0, MAX_ATTACHMENTS));
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  // while the agent works, a message waits for its next step, or interrupts it to be read right away
  function submit(event?: FormEvent, interrupt = false) {
    event?.preventDefault();
    if (!canSend) return;
    onSend(value.trim(), attachments, streaming && interrupt);
    setValue("");
    setAttachments([]);
    setError(null);
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) submit(event, event.metaKey || event.ctrlKey);
  }

  function onPaste(event: ClipboardEvent<HTMLTextAreaElement>) {
    const files = [...event.clipboardData.files];
    if (files.length === 0) return;
    event.preventDefault();
    void attach(files);
  }


  return (
    <form
      onSubmit={submit}
      className="glass rounded-2xl border border-line p-2 shadow-sm transition-colors focus-within:border-muted/50"
    >
      {queued.length > 0 && (
        <ul aria-label="Queued messages" className="mb-1 space-y-0.5 border-b border-line px-2 pb-2">
          {queued.map((message) => (
            <li key={message.id} className="flex items-center gap-2 text-xs text-muted">
              <LuClock size={12} className="shrink-0" />
              <span className="truncate">{message.content || message.attachments.map((file) => file.name).join(", ")}</span>
              <span className="ml-auto shrink-0">Queued</span>
            </li>
          ))}
        </ul>
      )}
      {attachments.length > 0 && (
        <div className="flex flex-wrap gap-2 px-2 pt-2 pb-1">
          {attachments.map((attachment, index) => (
            <AttachmentChip key={`${attachment.name}-${index}`} attachment={attachment} onRemove={() => setAttachments((current) => current.filter((_, at) => at !== index))} />
          ))}
        </div>
      )}
      <textarea
        ref={textarea}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={onKeyDown}
        onPaste={onPaste}
        rows={1}
        autoFocus
        aria-label="Message"
        placeholder={streaming ? "Add to the task" : "Ask anything"}
        className="block max-h-50 w-full resize-none bg-transparent px-2 py-1.5 text-sm leading-6 text-ink outline-none placeholder:text-muted"
      />
      {streaming && canSend && !error && (
        <p className="px-2 pb-1 text-[11px] text-muted">
          <kbd className="font-sans">Enter</kbd> adds it after the current step · <kbd className="font-sans">{isMac() ? "⌘" : "Ctrl"}+Enter</kbd> sends it now
        </p>
      )}
      {error && (
        <p role="alert" className="px-2 pb-1 text-xs text-danger">
          {error}
        </p>
      )}
      <div className="flex items-center gap-1">
        <div className="flex min-w-0 flex-1 items-center gap-0.5">
          <input
            ref={fileInput}
            type="file"
            multiple
            accept={ATTACHMENT_TYPES}
            className="hidden"
            onChange={(event) => {
              void attach([...(event.target.files ?? [])]);
              event.target.value = "";
            }}
          />
          <Button type="button" onClick={() => fileInput.current?.click()} aria-label="Attach files" title="Attach files" className={iconButton}>
            <LuPaperclip size={15} />
          </Button>
          <OptionSelect label="Model" options={MODELS} value={settings.model} onChange={(model) => onSettingsChange({ model })} />
          <OptionSelect label="Thinking effort" options={effortsFor(settings.model)} value={settings.effort} onChange={(effort) => onSettingsChange({ effort })} icon={<LuBrain size={13} className="shrink-0" />} />
          {supportsFast(settings.model) && <FastToggle pressed={settings.fast} onChange={(fast) => onSettingsChange({ fast })} />}
        </div>
        {streaming && canSend && (
          <Button
            type="button"
            onClick={() => submit(undefined, true)}
            title="Stop the current step and read this now (Ctrl+Enter)"
            className={`inline-flex h-8 cursor-pointer items-center rounded-md px-2.5 text-xs font-medium text-muted transition-colors hover:bg-subtle hover:text-ink ${focusRingOutset}`}
          >
            Send now
          </Button>
        )}
        {streaming && canSend ? (
          <Button type="submit" aria-label="Queue message" title="Send after the current step (Enter)" className={roundButton}>
            <LuArrowUp size={16} />
          </Button>
        ) : streaming ? (
          <Button type="button" onClick={onStop} aria-label="Stop" className={roundButton}>
            <LuSquare size={12} fill="currentColor" />
          </Button>
        ) : (
          <Button type="submit" disabled={!canSend} focusableWhenDisabled aria-label="Send" className={roundButton}>
            <LuArrowUp size={16} />
          </Button>
        )}
      </div>
    </form>
  );
}
