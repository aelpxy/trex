import { useRef, useState, type ChangeEvent } from "react";
import { LuFiles, LuUpload } from "react-icons/lu";

import { FileList } from "~/components/library/file-list";
import { Button } from "~/components/ui/button";
import { DeleteConfirmDialog, type DeleteTarget } from "~/components/ui/delete-confirm-dialog";
import { EmptyState } from "~/components/ui/empty-state";
import { Page } from "~/components/ui/page";
import { download, upload } from "~/lib/api";
import { pageTitle } from "~/lib/meta";
import { trex, type ApiFile } from "~/lib/trex";

import type { Route } from "./+types/library";

export const meta = () => pageTitle("Library");

export async function clientLoader() {
  return { files: await trex.files() };
}

export default function Library({ loaderData }: Route.ComponentProps) {
  const [files, setFiles] = useState<ApiFile[]>(loaderData.files);
  const [pending, setPending] = useState<DeleteTarget | null>(null);
  const [error, setError] = useState<string | null>(null);
  const [uploading, setUploading] = useState(false);
  const input = useRef<HTMLInputElement>(null);

  async function onPick(event: ChangeEvent<HTMLInputElement>) {
    const picked = [...(event.target.files ?? [])];
    event.target.value = "";
    if (picked.length === 0) return;
    setUploading(true);
    setError(null);
    try {
      await Promise.all(picked.map((file) => upload(file.name, file)));
      setFiles(await trex.files());
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setUploading(false);
    }
  }

  async function save(file: ApiFile) {
    try {
      const url = URL.createObjectURL(await download(file.path));
      Object.assign(document.createElement("a"), { href: url, download: file.path.split("/").pop() ?? "file" }).click();
      URL.revokeObjectURL(url);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    }
  }

  async function remove(target: DeleteTarget) {
    setPending(null);
    setFiles((current) => current.filter((file) => file.path !== target.id));
    try {
      await trex.deleteFile(target.id);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
      setFiles(await trex.files());
    }
  }

  const uploadButton = (
    <Button onClick={() => input.current?.click()} disabled={uploading}>
      <LuUpload size={14} />
      {uploading ? "Uploading…" : "Upload files"}
    </Button>
  );

  return (
    <Page title="Library">
      <input ref={input} type="file" multiple hidden onChange={onPick} />
      {error && <p role="alert" className="mt-4 text-sm text-danger">{error}</p>}
      {files.length === 0 ? (
        <EmptyState icon={LuFiles} title="No files yet" description="Upload files to use them across chats. Files the agent creates will show up here too." action={uploadButton} />
      ) : (
        <>
          <div className="mt-6 flex justify-end">{uploadButton}</div>
          <FileList files={files} onDownload={save} onDelete={(file) => setPending({ kind: "file", id: file.path, name: file.path })} />
        </>
      )}
      <DeleteConfirmDialog target={pending} onConfirm={remove} onCancel={() => setPending(null)} />
    </Page>
  );
}
