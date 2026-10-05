import type { ApiMe, ApiProject, ApiSession, ApiWorkspace } from "./trex";

export type Chat = { id: string; title: string };
export type Project = { id: string; name: string; chats: Chat[] };
export type Workspace = { id: string; name: string; plan: string; projects: Project[]; recents: Chat[] };

export type Account = { me: ApiMe; workspaceId: string; projects: ApiProject[]; sessions: ApiSession[] };

export const UNTITLED = "New chat";

export const chatOf = (session: ApiSession): Chat => ({ id: session.id, title: session.title ?? UNTITLED });

const planLabel = (plan: string) => plan.charAt(0).toUpperCase() + plan.slice(1);

// projects hold their chats; chats outside any project are the recents
export function workspaceOf(workspace: ApiWorkspace, projects: ApiProject[], sessions: ApiSession[]): Workspace {
  return {
    id: workspace.id,
    name: workspace.name,
    plan: planLabel(workspace.plan),
    projects: projects.map((project) => ({
      id: project.id,
      name: project.name,
      chats: sessions.filter((session) => session.project_id === project.id).map(chatOf),
    })),
    recents: sessions.filter((session) => !session.project_id || !projects.some((project) => project.id === session.project_id)).map(chatOf),
  };
}

export const emptyWorkspace = (workspace: ApiWorkspace): Workspace => workspaceOf(workspace, [], []);
