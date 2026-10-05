import { useSuspenseQuery } from "@tanstack/react-query";
import { useState } from "react";
import { useSearchParams } from "react-router";

import { FileList } from "~/components/library/file-list";
import { focusRing } from "~/components/ui/styles";
import { adminLibraryFile } from "~/lib/api";
import { queries } from "~/lib/queries";
import type { ApiAdminWorkspace, ApiFile } from "~/lib/trex";

import { FilterInput } from "./filter-input";
import { errorText } from "./format";
import { useUrlFilter } from "./use-url-filter";

// the workspace being browsed is in the url, so a link opens it directly; the picker lists the first
// page of workspaces matching the search, plus the one being browsed
function useBrowsedWorkspace() {
  const [params, setParams] = useSearchParams();
  const [search, setSearch] = useUrlFilter();
  const requested = params.get("workspace") ?? "";
  const { data: matching } = useSuspenseQuery(queries.admin.workspaces(1, search.trim()));
  const { data: browsed } = useSuspenseQuery(queries.admin.workspaces(1, requested));
  const current = (requested && browsed.data.find((candidate) => candidate.id === requested)) || matching.data[0] || null;
  const workspaces = current && !matching.data.some((candidate) => candidate.id === current.id) ? [current, ...matching.data] : matching.data;
  const browse = (id: string) =>
    setParams(
      (existing) => {
        const next = new URLSearchParams(existing);
        next.set("workspace", id);
        return next;
      },
      { replace: true, preventScrollReset: true },
    );
  return { workspaces, workspace: current, browse, search, setSearch, more: matching.total_count - matching.data.length };
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
  const { workspaces, workspace, browse, search, setSearch, more } = useBrowsedWorkspace();
  return (
    <div>
      <FilterInput value={search} onChange={setSearch} label="Find a workspace by name, member email or id" />
      {workspace ? (
        <WorkspaceFiles workspaces={workspaces} workspace={workspace} browse={browse} more={more} />
      ) : (
        <p className="mt-6 text-sm text-muted">{search ? "No workspace matches that search." : "No workspaces yet."}</p>
      )}
    </div>
  );
}

type WorkspaceFilesProps = { workspaces: ApiAdminWorkspace[]; workspace: ApiAdminWorkspace; browse: (id: string) => void; more: number };

function WorkspaceFiles({ workspaces, workspace, browse, more }: WorkspaceFilesProps) {
  return (
    <div className="mt-3">
      <select value={workspace.id} onChange={(event) => browse(event.target.value)} aria-label="Workspace" className={`ui-input h-10 ${focusRing}`}>
        {workspaces.map((option) => (
          <option key={option.id} value={option.id}>
            {option.name}
            {option.owner_email ? ` · ${option.owner_email}` : ""}
          </option>
        ))}
      </select>
      {more > 0 && <p className="mt-1.5 text-xs text-muted">{more.toLocaleString()} more match; search to narrow the list.</p>}
      <Files workspace={workspace.id} />
    </div>
  );
}
