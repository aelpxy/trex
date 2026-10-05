import { useRef, useState, type KeyboardEvent, type PointerEvent } from "react";

type Options = { initial: number; defaultValue: number; min: number; max: number; step: number; onCommit: (width: number) => void };

export function useResizableWidth({ initial, defaultValue, min, max, step, onCommit }: Options) {
  const clamp = (value: number) => Math.min(max, Math.max(min, value));
  const [width, setWidth] = useState(() => clamp(initial));
  const [resizing, setResizing] = useState(false);
  const drag = useRef<{ startX: number; startWidth: number } | null>(null);

  function commit(value: number) {
    setWidth(value);
    onCommit(value);
  }

  function onPointerDown(event: PointerEvent<HTMLElement>) {
    if (event.button !== 0) return;
    // keeps the browser from selecting text while dragging
    event.preventDefault();
    event.currentTarget.setPointerCapture(event.pointerId);
    drag.current = { startX: event.clientX, startWidth: width };
    setResizing(true);
  }

  function onPointerMove(event: PointerEvent<HTMLElement>) {
    if (!drag.current) return;
    setWidth(clamp(drag.current.startWidth + event.clientX - drag.current.startX));
  }

  function onPointerEnd() {
    if (!drag.current) return;
    drag.current = null;
    setResizing(false);
    onCommit(width);
  }

  function onKeyDown(event: KeyboardEvent<HTMLElement>) {
    const next: Record<string, number> = { ArrowLeft: width - step, ArrowRight: width + step, Home: min, End: max };
    if (!(event.key in next)) return;
    event.preventDefault();
    commit(clamp(next[event.key]));
  }

  return {
    width,
    resizing,
    reset: () => commit(defaultValue),
    handlers: { onPointerDown, onPointerMove, onPointerUp: onPointerEnd, onPointerCancel: onPointerEnd, onKeyDown },
  };
}
