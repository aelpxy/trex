import { version as REACT_VERSION } from "react";
import { transform } from "sucrase";

const ESM = "https://esm.sh";
const TAILWIND = "https://cdn.jsdelivr.net/npm/@tailwindcss/browser@4";
const SHARED = ["react", "react-dom"];

// bare imports load from esm.sh; react itself goes through the import map so every package shares one copy
function resolve(specifier: string) {
  if (/^(\.|\/|https?:)/.test(specifier)) return specifier;
  const root = specifier.startsWith("@") ? specifier.split("/").slice(0, 2).join("/") : specifier.split("/")[0];
  if (SHARED.includes(root)) return specifier;
  return `${ESM}/${specifier}?external=${SHARED.join(",")}`;
}

const rewriteImports = (code: string) =>
  code.replace(/(\bfrom\s*|\bimport\s*\(?\s*)(["'])([^"']+)\2/g, (_match, prefix: string, quote: string, specifier: string) => `${prefix}${quote}${resolve(specifier)}${quote}`);

// a page that renders a react component file's default export, styled with tailwind
export function reactPreview(source: string): string {
  let code: string;
  try {
    code = rewriteImports(transform(source, { transforms: ["jsx", "typescript"], jsxRuntime: "automatic", production: true }).code);
  } catch (error) {
    return errorPage(error instanceof Error ? error.message : String(error));
  }
  const react = `${ESM}/react@${REACT_VERSION}`;
  const dom = `${ESM}/react-dom@${REACT_VERSION}`;
  const imports = {
    react,
    "react/jsx-runtime": `${react}/jsx-runtime`,
    "react-dom": `${dom}?external=react`,
    "react-dom/client": `${dom}/client?external=react`,
  };
  return `<!doctype html>
<html>
<head>
<meta charset="utf-8">
<meta name="viewport" content="width=device-width, initial-scale=1">
<script type="importmap">${JSON.stringify({ imports }).replace(/</g, "\\u003c")}</script>
<script src="${TAILWIND}"></script>
</head>
<body>
<div id="root"></div>
<script type="module">
const show = (message) => { document.body.innerHTML = '<pre style="color:#b91c1c;padding:16px;white-space:pre-wrap;font:12px ui-monospace,monospace"></pre>'; document.body.firstChild.textContent = message; };
window.addEventListener("error", (event) => show(String(event.error?.stack ?? event.message)));
window.addEventListener("unhandledrejection", (event) => show(String(event.reason?.stack ?? event.reason)));
const source = ${JSON.stringify(code).replace(/</g, "\\u003c")};
const url = URL.createObjectURL(new Blob([source], { type: "text/javascript" }));
const [{ createElement }, { createRoot }, module] = await Promise.all([import("react"), import("react-dom/client"), import(url)]);
if (typeof module.default !== "function") show("The file has no default export to render.");
else createRoot(document.getElementById("root")).render(createElement(module.default));
</script>
</body>
</html>`;
}

function errorPage(message: string) {
  const escaped = message.replace(/&/g, "&amp;").replace(/</g, "&lt;");
  return `<!doctype html><pre style="color:#b91c1c;padding:16px;white-space:pre-wrap;font:12px ui-monospace,monospace">${escaped}</pre>`;
}
