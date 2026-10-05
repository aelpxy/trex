import { download } from "./api";

const SCHEME = /^[a-z][a-z0-9+.-]*:/i;

// the library path a reply's link points at: `library:` links, plus the sandbox paths and bare relative paths models sometimes write
export function libraryPath(href: string): string | null {
  let path: string;
  if (href.startsWith("library:")) path = href.slice("library:".length);
  else if (href.startsWith("sandbox:")) path = href.slice("sandbox:".length);
  else if (SCHEME.test(href) || href.startsWith("#") || href.startsWith("?") || href.startsWith("//")) return null;
  else path = href;
  path = path.replace(/^\/+/, "").replace(/^sandbox\//, "").replace(/^\.\//, "");
  if (!path) return null;
  try {
    return decodeURIComponent(path);
  } catch {
    return path;
  }
}

// library files are behind the bearer token, so they're fetched once and shown through object urls
const urls = new Map<string, Promise<string>>();

export function libraryUrl(path: string): Promise<string> {
  let url = urls.get(path);
  if (!url) {
    url = download(path).then((blob) => URL.createObjectURL(blob));
    url.catch(() => urls.delete(path));
    urls.set(path, url);
  }
  return url;
}

export async function saveLibraryFile(path: string) {
  const url = await libraryUrl(path);
  Object.assign(document.createElement("a"), { href: url, download: path.split("/").pop() ?? "file" }).click();
}
