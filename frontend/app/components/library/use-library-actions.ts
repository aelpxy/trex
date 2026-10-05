import { download } from "~/lib/api";
import { errorMessage, toasts, trackToast } from "~/lib/toasts";
import type { ApiFile } from "~/lib/trex";

import { baseName, filesOf, freeName, movesOf, openFile, parentOf, placeName, plural, type Entry } from "./entries";
import { useDeleteFiles, useMoveFiles, useUploadFiles } from "./mutations";

type Messages = { loading: string; success: string; error: string };

const describe = (entries: Entry[]) => (entries.length === 1 ? entries[0].name : plural(entries.length, "item"));

// every change to the library, each followed by a toast; `onDone` runs once one worked
export function useLibraryActions(files: ApiFile[], onDone: () => void) {
  const uploads = useUploadFiles();
  const moves = useMoveFiles();
  const deletes = useDeleteFiles();

  function run<T>(mutation: { mutateAsync: (items: T[]) => Promise<void> }, items: T[], messages: Messages) {
    trackToast(mutation.mutateAsync(items), messages).then(onDone, () => {});
  }

  // `keepBoth` gives files whose name is taken a numbered one instead of replacing
  function upload(folder: string, picked: File[], keepBoth = false) {
    if (picked.length === 0) return;
    const taken = new Set(files.map((file) => file.path));
    const items = picked.map((file) => {
      const name = keepBoth ? freeName(folder, file.name, taken) : file.name;
      taken.add(`${folder}${name}`);
      return { path: `${folder}${name}`, file };
    });
    const what = picked.length === 1 ? picked[0].name : plural(picked.length, "file");
    run(uploads, items, { loading: `Uploading ${what}…`, success: `Uploaded ${what} to ${placeName(folder)}`, error: `Couldn't upload ${what}` });
  }

  // names in `folder` that uploading `picked` would overwrite
  const conflicts = (folder: string, picked: File[]) => picked.map((file) => file.name).filter((name) => files.some((file) => file.path === `${folder}${name}`));

  function rename(entry: Entry, name: string) {
    run(moves, movesOf(entry, filesOf(entry, files), parentOf(entry.path), name), {
      loading: `Renaming ${entry.name}…`,
      success: `Renamed to ${name}`,
      error: `Couldn't rename ${entry.name}`,
    });
  }

  function move(entries: Entry[], folder: string) {
    const what = describe(entries);
    run(
      moves,
      entries.flatMap((entry) => movesOf(entry, filesOf(entry, files), folder)),
      { loading: `Moving ${what}…`, success: `Moved ${what} to ${placeName(folder)}`, error: `Couldn't move ${what}` },
    );
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
