import { isValidElement, useEffect, useState, type ComponentProps, type MouseEvent, type ReactElement } from "react";
import ReactMarkdown, { defaultUrlTransform, type Components } from "react-markdown";
import remarkGfm from "remark-gfm";

import { useOptionalFiles } from "~/components/files/files-provider";
import { libraryPath, libraryUrl, saveLibraryFile } from "~/lib/library-links";

import { CodeBlock } from "./code-block";

const LANGUAGE_CLASS = /language-([\w+#-]+)/;

function Pre({ children }: ComponentProps<"pre">) {
  const child = isValidElement(children) ? (children as ReactElement<{ className?: string; children?: unknown }>) : null;
  const code = String(child?.props.children ?? "").replace(/\n$/, "");
  const language = child?.props.className?.match(LANGUAGE_CLASS)?.[1];
  return <CodeBlock code={code} language={language} />;
}

const TEXT_FILE = /\.(html?|svg|md|markdown|txt|csv|json|xml|ya?ml|toml|css|[cm]?[jt]sx?|py|rs|go|sh|sql)$/i;

function Link({ href = "", children, node: _node, ...props }: ComponentProps<"a"> & { node?: unknown }) {
  const files = useOptionalFiles();
  const path = libraryPath(href);
  if (path) {
    // text files open in the files panel, where pages and markdown get a live preview; the rest download
    const save = (event: MouseEvent) => {
      event.preventDefault();
      const opened = files && TEXT_FILE.test(path) ? files.openLibrary(path) : saveLibraryFile(path);
      opened.catch((error) => console.warn("could not open the library file", error));
    };
    return (
      <a href="/library" {...props} onClick={save}>
        {children}
      </a>
    );
  }
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

function Image({ node: _node, alt = "", src, ...props }: ComponentProps<"img"> & { node?: unknown }) {
  const path = typeof src === "string" ? libraryPath(src) : null;
  const [url, setUrl] = useState<string | undefined>();
  useEffect(() => {
    if (!path) return;
    let current = true;
    libraryUrl(path)
      .then((next) => current && setUrl(next))
      .catch((error) => console.warn("could not load the library image", error));
    return () => {
      current = false;
    };
  }, [path]);
  if (path && !url) return <span className="text-muted">{alt}</span>;
  return <img alt={alt} loading="lazy" src={path ? url : src} {...props} />;
}

// library links have no http url, so they're kept for the components above instead of being stripped
const urlTransform = (url: string) => (libraryPath(url) ? url : defaultUrlTransform(url));

const COMPONENTS: Components = { pre: Pre, a: Link, table: Table, img: Image };
const REMARK_PLUGINS = [remarkGfm];

export function Markdown({ children, className = "text-sm leading-7" }: { children: string; className?: string }) {
  return (
    <div className={`markdown ${className}`}>
      <ReactMarkdown remarkPlugins={REMARK_PLUGINS} components={COMPONENTS} urlTransform={urlTransform}>
        {children}
      </ReactMarkdown>
    </div>
  );
}
