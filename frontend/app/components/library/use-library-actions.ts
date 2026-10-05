import { download } from "~/lib/api";
import { errorMessage, toasts, trackToast } from "~/lib/toasts";
import type { ApiFile } from "~/lib/trex";

import { baseName, filesOf, foldersOf, freeName, movesOf, openFile, parentOf, placeName, plural, type Entry } from "./entries";
import { useDeleteFiles, useMoveFiles, useUploadFiles } from "./mutations";

type Messages = { loading: string; success: string; error: string };

const describe = (entries: Entry[]) => (entries.length === 1 ? entries[0].name : plural(entries.length, "item"));

// every change to the library, each followed by a toast; `onDone` runs once one worked
export function useLibraryActions(files: ApiFile[], onDone: () => void) {
  const uploads = useUploadFiles();
  const mover = useMoveFiles();
  const deletes = useDeleteFiles();

  function run<T>(mutation: { mutateAsync: (items: T[]) => Promise<void> }, items: T[], messages: Messages) {
    trackToast(mutation.mutateAsync(items), messages).then(onDone, () => {});
  }

  // every path a file or folder already has; a new file can't take a folder's name or the reverse
  const taken = () => new Set([...files.map((file) => file.path), ...foldersOf(files).map((folder) => folder.slice(0, -1))]);

  // each entry lands at `folder` + its name; nothing starts if one would land on something that
  // exists or on another, so a folder is never merged into another or left half moved
  function relocate(entries: Entry[], folder: string, messages: Messages, name?: string) {
    const existing = taken();
    const landing = new Set<string>();
    const moving = entries.filter((entry) => parentOf(entry.path) !== folder || (name !== undefined && name !== baseName(entry.path)));
    for (const entry of moving) {
      const target = `${folder}${name ?? baseName(entry.path)}`;
      if (existing.has(target) || landing.has(target)) {
        toasts.add({ title: messages.error, description: `“${target}” already exists. Rename one of them first.`, type: "error" });
        return;
      }
      landing.add(target);
    }
    if (moving.length === 0) return;
    run(
      mover,
      moving.flatMap((entry) => movesOf(entry, filesOf(entry, files), folder, name)),
      messages,
    );
  }

  // `keepBoth` gives files whose name is taken a numbered one instead of replacing
  function upload(folder: string, picked: File[], keepBoth = false) {
    if (picked.length === 0) return;
    const used = taken();
    const items = picked.map((file) => {
      const name = keepBoth ? freeName(folder, file.name, used) : file.name;
      used.add(`${folder}${name}`);
      return { path: `${folder}${name}`, file };
    });
    const what = picked.length === 1 ? picked[0].name : plural(picked.length, "file");
    run(uploads, items, { loading: `Uploading ${what}…`, success: `Uploaded ${what} to ${placeName(folder)}`, error: `Couldn't upload ${what}` });
  }

  // names in `folder` that uploading `picked` would overwrite, or that a folder already has
  const conflicts = (folder: string, picked: File[]) => {
    const used = taken();
    return picked.map((file) => file.name).filter((name) => used.has(`${folder}${name}`));
  };

  function rename(entry: Entry, name: string) {
    const messages = { loading: `Renaming ${entry.name}…`, success: `Renamed to ${name}`, error: `Couldn't rename ${entry.name}` };
    relocate([entry], parentOf(entry.path), messages, name);
  }

  function move(entries: Entry[], folder: string) {
    const what = describe(entries);
    relocate(entries, folder, { loading: `Moving ${what}…`, success: `Moved ${what} to ${placeName(folder)}`, error: `Couldn't move ${what}` });
  }

  function remove(entries: Entry[]) {
    const what = describe(entries);
    run(
      deletes,
      entries.flatMap((entry) => filesOf(entry, files)),
      { loading: `Deleting ${what}…`, success: `Deleted ${what}`, error: `Couldn't delete ${what}` },
    );
  }

  function show(entry: Entry, asDownload: boolean) {
    openFile(() => download(entry.path), entry.path, asDownload).catch((cause) =>
      toasts.add({ title: `Couldn't ${asDownload ? "download" : "open"} ${baseName(entry.path)}`, description: errorMessage(cause), type: "error" }),
    );
  }

  return { upload, conflicts, rename, move, remove, show };
}
