import { useCallback, useEffect, useRef, useState } from "react";

const FOLLOW_THRESHOLD_PX = 80;

const isAtBottom = (element: HTMLElement) => element.scrollTop + element.clientHeight >= element.scrollHeight - FOLLOW_THRESHOLD_PX;

export function useFollowScroll(content: unknown) {
  const scroller = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const [atBottom, setAtBottom] = useState(true);

  useEffect(() => {
    const element = scroller.current;
    if (!element) return;
    const onScroll = () => {
      following.current = isAtBottom(element);
      setAtBottom(following.current);
    };
    element.addEventListener("scroll", onScroll, { passive: true });
    return () => element.removeEventListener("scroll", onScroll);
  }, []);

  useEffect(() => {
    const element = scroller.current;
    if (!element) return;
    if (following.current) element.scrollTop = element.scrollHeight;
    else setAtBottom(isAtBottom(element));
  }, [content]);

  const follow = useCallback(() => {
    following.current = true;
  }, []);

  const scrollToBottom = useCallback(() => {
    following.current = true;
    scroller.current?.scrollTo({ top: scroller.current.scrollHeight, behavior: "smooth" });
  }, []);

  return { scroller, atBottom, follow, scrollToBottom };
}
