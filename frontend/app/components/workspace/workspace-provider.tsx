import { createContext, use, useCallback, useMemo, useState, type ReactNode } from "react";
import { useLocation, useNavigate } from "react-router";

import { DeleteConfirmDialog, type DeleteTarget } from "~/components/ui/delete-confirm-dialog";
import { setWorkspaceId } from "~/lib/api";
import { queryClient } from "~/lib/query-client";
import { trex, type ApiProject, type ApiSession, type ApiUser } from "~/lib/trex";
import { useUiState } from "~/lib/ui-state";
import { emptyWorkspace, workspaceOf, type Account, type Workspace } from "~/lib/workspace";

type WorkspaceContextValue = {
  workspaces: Workspace[];
  current: Workspace;
  profile: ApiUser;
  switchWorkspace: (id: string) => void;
  projects: Workspace["projects"];
  recents: Workspace["recents"];
  requestDelete: (target: DeleteTarget) => void;
  createProject: (name: string) => Promise<void>;
  addChat: (session: ApiSession) => void;
  renameChat: (id: string, title: string) => void;
  logout: () => void;
};

const WorkspaceContext = createContext<WorkspaceContextValue | null>(null);

export function useWorkspace() {
  const value = use(WorkspaceContext);
  if (!value) throw new Error("useWorkspace must be used inside WorkspaceProvider");
  return value;
}

const chatCountPhrase = (count: number) => (count > 0 ? ` and its ${count} ${count === 1 ? "chat" : "chats"}` : "");

// the signed-in account's workspaces, with the current one's projects and chats from the trex api
export function WorkspaceProvider({ account, children }: { account: Account; children: ReactNode }) {
  const [workspaceId, setCurrentId] = useState(account.workspaceId);
  const [projects, setProjects] = useState<ApiProject[]>(account.projects);
  const [sessions, setSessions] = useState<ApiSession[]>(account.sessions);
  const [pending, setPending] = useState<DeleteTarget | null>(null);
  const { update } = useUiState();
  const location = useLocation();
  const navigate = useNavigate();

  const apiWorkspace = account.me.workspaces.find((workspace) => workspace.id === workspaceId) ?? account.me.workspaces[0];
  const current = useMemo(() => workspaceOf(apiWorkspace, projects, sessions), [apiWorkspace, projects, sessions]);
  const workspaces = useMemo(
    () => account.me.workspaces.map((workspace) => (workspace.id === current.id ? current : emptyWorkspace(workspace))),
    [account.me.workspaces, current],
  );

  const switchWorkspace = useCallback(
    (id: string) => {
      setWorkspaceId(id);
      setCurrentId(id);
      update({ workspaceId: id });
      setProjects([]);
      setSessions([]);
      navigate("/");
      Promise.all([trex.projects(), trex.sessions()])
        .then(([nextProjects, nextSessions]) => {
          setProjects(nextProjects);
          setSessions(nextSessions);
        })
        .catch((error) => console.warn("could not load the workspace", error));
    },
    [update, navigate],
  );

  const createProject = useCallback(async (name: string) => {
    const project = await trex.createProject(name);
    setProjects((current) => [project, ...current]);
  }, []);

  const addChat = useCallback((session: ApiSession) => setSessions((current) => [session, ...current.filter((existing) => existing.id !== session.id)]), []);

  const renameChat = useCallback((id: string, title: string) => setSessions((current) => current.map((session) => (session.id === id ? { ...session, title } : session))), []);

  const logout = useCallback(() => {
    trex
      .logout()
      .catch((error) => console.warn("could not end the session", error))
      .finally(() => {
        setWorkspaceId(null);
        queryClient.clear();
        navigate("/auth", { replace: true });
      });
  }, [navigate]);

  const requestDelete = useCallback((target: DeleteTarget) => setPending(target), []);

  const chatIdsOf = (target: DeleteTarget) =>
    target.kind === "project" ? (current.projects.find((project) => project.id === target.id)?.chats.map((chat) => chat.id) ?? []) : [target.id];

  // a deleted project takes its chats with it, as the confirmation says
  async function confirmDelete(target: DeleteTarget) {
    const chatIds = chatIdsOf(target);
    setPending(null);
    if (chatIds.some((id) => location.pathname === `/chat/${id}`)) navigate("/");
    setSessions((current) => current.filter((session) => !chatIds.includes(session.id)));
    if (target.kind === "project") setProjects((current) => current.filter((project) => project.id !== target.id));
    try {
      await Promise.all(chatIds.map((id) => trex.deleteSession(id)));
      if (target.kind === "project") await trex.deleteProject(target.id);
    } catch (error) {
      console.warn(`could not delete the ${target.kind}`, error);
      const [nextProjects, nextSessions] = await Promise.all([trex.projects(), trex.sessions()]);
      setProjects(nextProjects);
      setSessions(nextSessions);
    }
  }

  const value = useMemo(
    () => ({
      workspaces,
      current,
      profile: account.me.user,
      switchWorkspace,
      projects: current.projects,
      recents: current.recents,
      requestDelete,
      createProject,
      addChat,
      renameChat,
      logout,
    }),
    [workspaces, current, account.me.user, switchWorkspace, requestDelete, createProject, addChat, renameChat, logout],
  );

  return (
    <WorkspaceContext value={value}>
      {children}
      <DeleteConfirmDialog
        target={pending}
        consequence={chatCountPhrase(pending?.kind === "project" ? chatIdsOf(pending).length : 0)}
        onConfirm={confirmDelete}
        onCancel={() => setPending(null)}
      />
    </WorkspaceContext>
  );
}
