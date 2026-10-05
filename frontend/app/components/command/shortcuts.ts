import { useEffect } from "react";
import { useNavigate } from "react-router";

export const OPEN_SEARCH_EVENT = "open-search";

export const isMac = () => typeof navigator !== "undefined" && /Mac|iPhone|iPad/.test(navigator.platform);
// phones and tablets, where enter is for new lines and focusing a field opens the keyboard
export const isTouch = () => typeof window !== "undefined" && window.matchMedia("(pointer: coarse)").matches;

export const openSearch = () => window.dispatchEvent(new Event(OPEN_SEARCH_EVENT));

const isTyping = (target: EventTarget | null) =>
  target instanceof HTMLElement && (target.isContentEditable || ["INPUT", "TEXTAREA", "SELECT"].includes(target.tagName));

// app-wide keys; ⌘K is handled by the palette itself
export function useGlobalShortcuts() {
  const navigate = useNavigate();
  useEffect(() => {
    const onKey = (event: KeyboardEvent) => {
      const mod = event.metaKey || event.ctrlKey;
      if (mod && event.shiftKey && event.key.toLowerCase() === "o") {
        event.preventDefault();
        navigate("/");
      } else if (event.key === "/" && !mod && !isTyping(event.target)) {
        const composer = document.querySelector<HTMLTextAreaElement>('textarea[aria-label="Message"]');
        if (!composer) return;
        event.preventDefault();
        composer.focus();
      }
    };
    window.addEventListener("keydown", onKey);
    return () => window.removeEventListener("keydown", onKey);
  }, [navigate]);
}
