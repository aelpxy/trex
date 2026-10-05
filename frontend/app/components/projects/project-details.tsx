import { useState } from "react";
import { Link } from "react-router";
import { LuMessageSquare, LuPencil, LuTrash2 } from "react-icons/lu";

import { Button } from "~/components/ui/button";
import { focusRing, iconButton } from "~/components/ui/styles";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import type { Project } from "~/lib/workspace";

const MAX_INSTRUCTIONS = 20_000;

function Instructions({ project }: { project: Project }) {
  const { updateProject } = useWorkspace();
  const [editing, setEditing] = useState(false);
  const [draft, setDraft] = useState(project.instructions ?? "");
  const [saving, setSaving] = useState(false);
  const [error, setError] = useState<string>();

  async function save() {
    setSaving(true);
    setError(undefined);
    try {
      await updateProject(project.id, { instructions: draft.trim() || null });
      setEditing(false);
    } catch (cause) {
      setError(cause instanceof Error ? cause.message : String(cause));
    } finally {
      setSaving(false);
    }
  }

  return (
    <section className="ui-card px-5 py-4">
      <div className="flex items-center gap-3">
        <div className="min-w-0 flex-1">
          <h2 className="text-sm font-medium">Instructions</h2>
          <p className="mt-0.5 text-xs text-muted">Every chat in this project follows them.</p>
        </div>
        {!editing && (
          <Button
            variant="quiet"
            onClick={() => {
              setDraft(project.instructions ?? "");
              setEditing(true);
            }}
          >
            <LuPencil size={13} />
            Edit
          </Button>
        )}
      </div>
      {editing ? (
        <>
          <textarea
            autoFocus
            value={draft}
            maxLength={MAX_INSTRUCTIONS}
            onChange={(event) => setDraft(event.target.value)}
            placeholder="Use TypeScript and pnpm. Keep answers short."
            aria-label="Project instructions"
            className={`ui-input mt-3 h-auto min-h-36 resize-y py-2.5 leading-6 ${focusRing}`}
          />
          {error && (
            <p role="alert" className="mt-2 text-xs text-danger">
              {error}
            </p>
          )}
          <div className="mt-3 flex justify-end gap-2">
            <Button variant="quiet" onClick={() => setEditing(false)}>
              Cancel
            </Button>
            <Button onClick={save} disabled={saving}>
              Save
            </Button>
          </div>
        </>
      ) : (
        <p className={`mt-3 text-sm leading-6 whitespace-pre-wrap ${project.instructions ? "" : "text-muted"}`}>{project.instructions || "No instructions yet."}</p>
      )}
    </section>
  );
}

function RenameProject({ project, onDone }: { project: Project; onDone: () => void }) {
  const { updateProject } = useWorkspace();

  function commit(value: string) {
    const name = value.trim();
    if (name && name !== project.name) updateProject(project.id, { name }).catch((error) => console.warn("could not rename the project", error));
    onDone();
  }

  return (
    <input
      autoFocus
      defaultValue={project.name}
      aria-label="Project name"
      onFocus={(event) => event.currentTarget.select()}
      onBlur={(event) => commit(event.currentTarget.value)}
      onKeyDown={(event) => {
        if (event.key === "Enter") commit(event.currentTarget.value);
        if (event.key === "Escape") onDone();
      }}
      className="h-8 min-w-0 flex-1 rounded-md bg-surface px-2 text-sm font-medium ring-1 ring-line outline-none focus:ring-muted/50"
    />
  );
}

// shown under a project's composer: its name, instructions and chats
export function ProjectDetails({ project }: { project: Project }) {
  const { requestDelete } = useWorkspace();
  const [renaming, setRenaming] = useState(false);

  return (
    <div className="mt-8 space-y-6">
      <div className="flex h-8 items-center gap-1">
        {renaming ? (
          <RenameProject project={project} onDone={() => setRenaming(false)} />
        ) : (
          <>
            <span className="min-w-0 flex-1 truncate text-xs text-muted">Project · {project.name}</span>
            <button type="button" aria-label="Rename project" title="Rename project" onClick={() => setRenaming(true)} className={iconButton}>
              <LuPencil size={14} />
            </button>
            <button type="button" aria-label="Delete project" title="Delete project" onClick={() => requestDelete({ kind: "project", id: project.id, name: project.name })} className={`${iconButton} hover:text-danger`}>
              <LuTrash2 size={14} />
            </button>
          </>
        )}
      </div>
      <Instructions key={project.id} project={project} />
      <section>
        <h2 className="text-sm font-medium">Chats</h2>
        {project.chats.length === 0 ? (
          <p className="mt-2 text-sm text-muted">No chats yet. Start one above, or move a chat here from its menu.</p>
        ) : (
          <ul className="ui-card mt-3 divide-y divide-line overflow-hidden">
            {project.chats.map((chat) => (
              <li key={chat.id}>
                <Link to={`/chat/${chat.id}`} className={`flex items-center gap-3 px-4 py-2.5 text-sm hover:bg-subtle/60 ${focusRing}`}>
                  <LuMessageSquare size={14} className="shrink-0 text-muted" />
                  <span className="min-w-0 flex-1 truncate">{chat.title}</span>
                </Link>
              </li>
            ))}
          </ul>
        )}
      </section>
    </div>
  );
}
