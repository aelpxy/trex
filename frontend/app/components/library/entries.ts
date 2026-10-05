import type { IconType } from "react-icons";
import { LuFile, LuFileCode, LuFileImage, LuFileText, LuFolder } from "react-icons/lu";

import type { ApiFile } from "~/lib/trex";

const UNITS = ["B", "KB", "MB", "GB"];

export function formatSize(bytes: number) {
  let size = bytes;
  let unit = 0;
  while (size >= 1024 && unit < UNITS.length - 1) {
    size /= 1024;
    unit += 1;
  }
  return `${unit === 0 ? size : size.toFixed(1)} ${UNITS[unit]}`;
}

const IMAGE = /\.(png|jpe?g|gif|webp|svg|avif)$/i;
const CODE = /\.(js|jsx|ts|tsx|py|rs|go|json|ya?ml|toml|sh|css|html?|sql)$/i;
const TEXT = /\.(md|txt|csv|log|pdf)$/i;
// what a browser can show in a tab, rather than only download
const VIEWABLE = /\.(png|jpe?g|gif|webp|svg|avif|pdf|txt|md|csv|json|log)$/i;

export const viewable = (name: string) => VIEWABLE.test(name);

// a folder or file directly inside the folder being browsed; folders only exist as path prefixes
export type Entry = { kind: "folder" | "file"; name: string; path: string; size: number; modified: number; files: number };

export const entryIcon = (entry: Entry): IconType =>
  entry.kind === "folder" ? LuFolder : IMAGE.test(entry.name) ? LuFileImage : CODE.test(entry.name) ? LuFileCode : TEXT.test(entry.name) ? LuFileText : LuFile;

export const baseName = (path: string) => path.replace(/\/$/, "").split("/").pop() ?? path;

// the folders and files one level under `folder`, or every file matching `search` anywhere
export function entriesOf(files: ApiFile[], folder: string, search: string): Entry[] {
  const needle = search.trim().toLowerCase();
  if (needle) {
    return files.filter((file) => file.path.toLowerCase().includes(needle)).map((file) => ({ kind: "file", name: file.path, path: file.path, size: file.size, modified: file.modified_at, files: 1 }));
  }
  const folders = new Map<string, Entry>();
  const entries: Entry[] = [];
  for (const file of files) {
    if (!file.path.startsWith(folder)) continue;
    const rest = file.path.slice(folder.length);
    const slash = rest.indexOf("/");
    if (slash === -1) {
      entries.push({ kind: "file", name: rest, path: file.path, size: file.size, modified: file.modified_at, files: 1 });
      continue;
    }
    const name = rest.slice(0, slash);
    const existing = folders.get(name) ?? { kind: "folder", name, path: `${folder}${name}/`, size: 0, modified: 0, files: 0 };
    folders.set(name, { ...existing, size: existing.size + file.size, modified: Math.max(existing.modified, file.modified_at), files: existing.files + 1 });
  }
  return [...[...folders.values()].sort((a, b) => a.name.localeCompare(b.name)), ...entries.sort((a, b) => a.name.localeCompare(b.name))];
}

// every folder that holds a file, at any depth, as `a/` and `a/b/`
export function foldersOf(files: ApiFile[]): string[] {
  const folders = new Set<string>();
  for (const file of files) {
    const parts = file.path.split("/").slice(0, -1);
    parts.forEach((_, index) => folders.add(`${parts.slice(0, index + 1).join("/")}/`));
  }
  return [...folders].sort();
}

// the files an entry stands for: itself, or everything under a folder
export const filesOf = (entry: Entry, files: ApiFile[]) => (entry.kind === "file" ? [entry.path] : files.filter((file) => file.path.startsWith(entry.path)).map((file) => file.path));

// a blob url, since files need the session cookie; the tab opens before the fetch so popup blockers allow it
export async function openFile(load: () => Promise<Blob>, path: string, download: boolean) {
  const tab = download ? null : window.open("", "_blank");
  try {
    const url = URL.createObjectURL(await load());
    if (tab) tab.location.href = url;
    else Object.assign(document.createElement("a"), { href: url, download: baseName(path) }).click();
    setTimeout(() => URL.revokeObjectURL(url), 60_000);
  } catch (cause) {
    tab?.close();
    throw cause;
  }
}
