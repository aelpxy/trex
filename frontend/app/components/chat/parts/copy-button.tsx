import { useEffect, useRef, useState } from "react";
import { Button } from "@base-ui/react/button";
import { LuCheck, LuCopy } from "react-icons/lu";

import { iconButton } from "~/components/ui/styles";

const COPIED_RESET_MS = 1500;

export function CopyButton({ text, label = "Copy" }: { text: string; label?: string }) {
  const [copied, setCopied] = useState(false);
  const timer = useRef<ReturnType<typeof setTimeout> | null>(null);
  useEffect(() => () => {
    if (timer.current) clearTimeout(timer.current);
  }, []);

  async function copy() {
    try {
      await navigator.clipboard.writeText(text);
      setCopied(true);
      if (timer.current) clearTimeout(timer.current);
      timer.current = setTimeout(() => setCopied(false), COPIED_RESET_MS);
    } catch (error) {
      console.warn("could not copy", error);
    }
  }

  return (
    <Button onClick={copy} aria-label={copied ? "Copied" : label} title={copied ? "Copied" : label} className={`${iconButton} size-7`}>
      {copied ? <LuCheck size={13} /> : <LuCopy size={13} />}
    </Button>
  );
}
