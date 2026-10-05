import { useLayoutEffect, useRef, useState, type KeyboardEvent } from "react";

import { Button } from "~/components/ui/button";

const MAX_HEIGHT_PX = 240;

export function MessageEditor({ initial, onSave, onCancel }: { initial: string; onSave: (content: string) => Promise<void>; onCancel: () => void }) {
  const [value, setValue] = useState(initial);
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const textarea = useRef<HTMLTextAreaElement>(null);

  useLayoutEffect(() => {
    const element = textarea.current;
    if (!element) return;
    element.style.height = "auto";
    element.style.height = `${Math.min(element.scrollHeight, MAX_HEIGHT_PX)}px`;
  }, [value]);

  async function save() {
    if (!value.trim() || saving) return;
    setSaving(true);
    setError(null);
    try {
      await onSave(value.trim());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setSaving(false);
    }
  }

  function onKeyDown(event: KeyboardEvent<HTMLTextAreaElement>) {
    if (event.key === "Escape") onCancel();
    if (event.key === "Enter" && !event.shiftKey && !event.nativeEvent.isComposing) {
      event.preventDefault();
      void save();
    }
  }

  return (
    <div className="w-full max-w-[85%] rounded-2xl border border-line bg-subtle/60 p-2">
      <textarea
        ref={textarea}
        value={value}
        onChange={(event) => setValue(event.target.value)}
        onKeyDown={onKeyDown}
        autoFocus
        aria-label="Edit message"
        className="block w-full resize-none bg-transparent px-2 py-1.5 text-sm leading-6 text-ink outline-none"
      />
      {error && (
        <p role="alert" className="px-2 pb-1 text-xs text-danger">
          {error}
        </p>
      )}
      <div className="flex items-center justify-end gap-2 px-1 pt-1">
        <span className="mr-auto px-1 text-[11px] text-muted">Continues in a new chat</span>
        <Button variant="quiet" onClick={onCancel} className="h-7 px-2.5 text-xs">
          Cancel
        </Button>
        <Button onClick={() => void save()} disabled={!value.trim() || saving} className="h-7 px-2.5 text-xs">
          {saving ? "Sending…" : "Send"}
        </Button>
      </div>
    </div>
  );
}
