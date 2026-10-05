import { isValidElement, type ComponentProps, type ReactElement } from "react";
import ReactMarkdown, { type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { CodeBlock } from "./code-block";

const LANGUAGE_CLASS = /language-([\w+#-]+)/;

function Pre({ children }: ComponentProps<"pre">) {
  const child = isValidElement(children) ? (children as ReactElement<{ className?: string; children?: unknown }>) : null;
  const code = String(child?.props.children ?? "").replace(/\n$/, "");
  const language = child?.props.className?.match(LANGUAGE_CLASS)?.[1];
  return <CodeBlock code={code} language={language} />;
}

function Link({ href = "", children, node: _node, ...props }: ComponentProps<"a"> & { node?: unknown }) {
  const external = /^https?:\/\//.test(href);
  return (
    <a href={href} {...props} {...(external && { target: "_blank", rel: "noopener noreferrer" })}>
      {children}
    </a>
  );
}

function Table({ node: _node, ...props }: ComponentProps<"table"> & { node?: unknown }) {
  return (
    <div className="overflow-x-auto rounded-lg border border-line">
      <table {...props} />
    </div>
  );
}

function Image({ node: _node, alt = "", ...props }: ComponentProps<"img"> & { node?: unknown }) {
  return <img alt={alt} loading="lazy" {...props} />;
}

const COMPONENTS: Components = { pre: Pre, a: Link, table: Table, img: Image };
const REMARK_PLUGINS = [remarkGfm];

export function Markdown({ children, className = "text-sm leading-7" }: { children: string; className?: string }) {
  return (
    <div className={`markdown ${className}`}>
      <ReactMarkdown remarkPlugins={REMARK_PLUGINS} components={COMPONENTS}>
        {children}
      </ReactMarkdown>
    </div>
  );
}
