export type Preview = "markdown" | "html" | "react";

// files the panel can render as well as show as code
export function previewOf(path: string): Preview | null {
  if (/\.(md|markdown)$/i.test(path)) return "markdown";
  if (/\.(html?|svg)$/i.test(path)) return "html";
  if (/\.[jt]sx$/i.test(path)) return "react";
  return null;
}
