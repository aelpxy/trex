import { useSuspenseQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useSearchParams } from "react-router";

import { FileList } from "~/components/library/file-list";
import { focusRing } from "~/components/ui/styles";
import { adminLibraryFile } from "~/lib/api";
import { queries } from "~/lib/queries";
import type { ApiFile } from "~/lib/trex";

import { errorText } from "./format";

// the workspace being browsed is in the url, so a link opens it directly
export function useBrowsedWorkspace() {
  const { data: workspaces } = useSuspenseQuery(queries.admin.workspaces());
  const [params, setParams] = useSearchParams();
  const requested = params.get("workspace");
  const workspace = workspaces.find((candidate) => candidate.id === requested) ?? workspaces[0] ?? null;
  const browse = (id: string) => setParams({ workspace: id }, { replace: true, preventScrollReset: true });
  return { workspaces, workspace, browse };
}

function Files({ workspace }: { workspace: string }) {
  const { data: files } = useSuspenseQuery(queries.admin.library(workspace));
  const [error, setError] = useState<string | null>(null);

  async function save(file: ApiFile) {
    setError(null);
    try {
      const url = URL.createObjectURL(await adminLibraryFile(workspace, file.path));
      Object.assign(document.createElement("a"), { href: url, download: file.path.split("/").pop() ?? "file" }).click();
      URL.revokeObjectURL(url);
    } catch (cause) {
      setError(errorText(cause));
    }
  }

  if (files.length === 0) return <p className="mt-6 text-sm text-muted">This workspace's library is empty.</p>;
  return (
    <>
      {error && <p role="alert" className="mt-3 text-sm text-danger">{error}</p>}
      <FileList files={files} onDownload={(file) => void save(file)} />
    </>
  );
}

export function WorkspaceLibrary() {
  const { workspaces, workspace, browse } = useBrowsedWorkspace();
  if (!workspace) return <p className="text-sm text-muted">No workspaces yet.</p>;
  return (
    <div>
      <select value={workspace.id} onChange={(event) => browse(event.target.value)} aria-label="Workspace" className={`ui-input h-10 ${focusRing}`}>
        {workspaces.map((option) => (
          <option key={option.id} value={option.id}>
            {option.name}
            {option.owner_email ? ` · ${option.owner_email}` : ""}
          </option>
        ))}
      </select>
      <Files workspace={workspace.id} />
    </div>
  );
}
