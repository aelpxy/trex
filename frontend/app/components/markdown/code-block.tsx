import { useEffect, useRef, useState, type CSSProperties } from "react";
import { Button } from "@base-ui/react/button";
import { LuCheck, LuCopy } from "react-icons/lu";

import { focusRing } from "~/components/ui/styles";

const COPIED_RESET_MS = 1500;
const THEMES = { light: "github-light", dark: "github-dark" };

type CodeBlockProps = { code: string; language?: string; lineNumbers?: boolean };

export function CodeBlock({ code, language, lineNumbers = false }: CodeBlockProps) {
  const [html, setHtml] = useState<string | null>(null);
  const [copied, setCopied] = useState(false);
  const resetTimer = useRef<ReturnType<typeof setTimeout> | null>(null);

  useEffect(() => {
    let cancelled = false;
    // loaded on demand so chats without code never download the highlighter
    import("shiki")
      .then(({ bundledLanguages, codeToHtml }) => {
        const lang = language && language in bundledLanguages ? language : "text";
        return codeToHtml(code, { lang, themes: THEMES, defaultColor: false });
      })
      .then((result) => !cancelled && setHtml(result))
      .catch((error) => console.warn("could not highlight code block", error));
    return () => {
      cancelled = true;
    };
  }, [code, language]);

  useEffect(() => () => {
    if (resetTimer.current) clearTimeout(resetTimer.current);
  }, []);

  async function copy() {
    try {
      await navigator.clipboard.writeText(code);
      setCopied(true);
      if (resetTimer.current) clearTimeout(resetTimer.current);
      resetTimer.current = setTimeout(() => setCopied(false), COPIED_RESET_MS);
    } catch (error) {
      console.warn("could not copy code block", error);
    }
  }

  return (
    <div
      className={`code-block overflow-hidden rounded-xl border border-line bg-surface [&_pre]:max-h-[32rem] [&_pre]:overflow-auto [&_pre]:p-4 [&_pre]:font-mono [&_pre]:text-xs [&_pre]:leading-6 ${lineNumbers ? "line-numbers" : ""}`}
      style={{ "--gutter": `${String(code.split("\n").length).length}ch` } as CSSProperties}
    >
      <div className="flex h-9 items-center justify-between border-b border-line pr-1.5 pl-4">
        <span className="font-mono text-[11px] text-muted">{language ?? "text"}</span>
        <Button
          onClick={copy}
          aria-label={copied ? "Copied" : "Copy code"}
          className={`inline-flex h-7 cursor-pointer items-center gap-1.5 rounded-md px-2 text-xs text-muted transition-colors hover:bg-subtle hover:text-ink ${focusRing}`}
        >
          {copied ? <LuCheck size={13} /> : <LuCopy size={13} />}
          <span aria-live="polite">{copied ? "Copied" : "Copy"}</span>
        </Button>
      </div>
      {html ? (
        <div dangerouslySetInnerHTML={{ __html: html }} />
      ) : (
        <pre>
          <code>
            {code.split("\n").map((line, index) => (
              <span key={index}>
                {index > 0 && "\n"}
                <span className="line">{line}</span>
              </span>
            ))}
          </code>
        </pre>
      )}
    </div>
  );
}
