import { api, encodePath } from "./api";

// shapes of the trex api, see /docs on the trex server

export type ApiUser = { id: string; email: string; name: string; created_at: number };
export type ApiWorkspace = { id: string; name: string; plan: string; role: string; credits: number; created_at: number };
export type ApiToken = { token: string; user: ApiUser; workspaces: ApiWorkspace[] };
export type ApiMe = { user: ApiUser; workspaces: ApiWorkspace[] };
export type List<T> = { object: "list"; data: T[]; has_more: boolean };

export type ApiModel = {
  id: string;
  name: string;
  context_window: number;
  reasoning_efforts: string[] | null;
  fast: boolean;
  price: { input: number; cached_input: number; output: number; fast_multiplier: number };
};

export type ApiQuestion = { question: string; options: { label: string; description: string | null }[]; multi_select: boolean };

export type ApiSession = {
  id: string;
  title: string | null;
  project_id: string | null;
  model: string;
  reasoning_effort: string | null;
  fast: boolean;
  status: "idle" | "running" | "needs_input" | "failed";
  pending_questions: ApiQuestion[] | null;
  last_error: string | null;
  created_at: number;
  updated_at: number;
};

export type ApiProject = { id: string; name: string; instructions: string | null; created_at: number; updated_at: number };

export type ApiItem = (
  | { type: "message"; role: string; text: string; attachments: { id: string; kind: string; mime_type: string; filename: string | null }[] }
  | { type: "tool_call"; call_id: string; name: string; arguments: string }
  | { type: "tool_result"; call_id: string; output: string }
  | { type: "reasoning"; summary: string }
  | { type: "compaction"; summary: string }
) & { seq: number; created_at: number };

export type ApiUsage = {
  created_at: number;
  model: string;
  input_tokens: number;
  cached_input_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  credits: number;
  duration_ms: number;
};

export type ApiAttachmentInput = { data: string; filename: string };

export type ApiAccessRequest = { id: string; status: string; endpoints: string[]; binary: string; rationale: string };

export type ApiSandboxFile = { path: string; size: number; modified_at: number };

export type ApiFile = { path: string; size: number; modified_at: number };

export type ApiCredits = { balance: number; plan: { id: string; name: string; monthly_credits: number } | null; enforced: boolean };

const MAX_PAGE = 100;

// every page, since the sidebar shows all chats and projects
async function all<T extends { id: string }>(path: string): Promise<T[]> {
  const items: T[] = [];
  const separator = path.includes("?") ? "&" : "?";
  for (let cursor: string | null = null; ; ) {
    const page: List<T> = await api<List<T>>("GET", `${path}${separator}limit=${MAX_PAGE}${cursor ? `&starting_after=${cursor}` : ""}`);
    items.push(...page.data);
    if (!page.has_more || page.data.length === 0) return items;
    cursor = page.data[page.data.length - 1].id;
  }
}

export const trex = {
  signup: (body: { name: string; email: string; password: string }) => api<ApiToken>("POST", "/auth/signup", body),
  login: (body: { email: string; password: string }) => api<ApiToken>("POST", "/auth/login", body),
  logout: () => api<void>("POST", "/auth/logout"),
  me: () => api<ApiMe>("GET", "/me"),
  models: () => api<List<ApiModel>>("GET", "/models").then((list) => list.data),
  credits: () => api<ApiCredits>("GET", "/credits"),

  projects: () => all<ApiProject>("/projects"),
  createProject: (name: string) => api<ApiProject>("POST", "/projects", { name }),
  deleteProject: (id: string) => api<unknown>("DELETE", `/projects/${id}`),

  sessions: () => all<ApiSession>("/sessions"),
  session: (id: string) => api<ApiSession>("GET", `/sessions/${id}`),
  createSession: (body: { model: string; reasoning_effort: string | null; fast: boolean; project_id?: string | null }) => api<ApiSession>("POST", "/sessions", body),
  updateSession: (id: string, body: { title?: string; project_id?: string | null; model?: string; reasoning_effort?: string | null; fast?: boolean }) => api<ApiSession>("PATCH", `/sessions/${id}`, body),
  deleteSession: (id: string) => api<unknown>("DELETE", `/sessions/${id}`),
  items: (id: string) => api<List<ApiItem>>("GET", `/sessions/${id}/items`).then((list) => list.data),
  usage: (id: string) => api<List<ApiUsage>>("GET", `/sessions/${id}/usage`).then((list) => list.data),
  sendMessage: (id: string, content: string, options: { interrupt?: boolean; attachments?: ApiAttachmentInput[] } = {}) =>
    api<{ queued: boolean }>("POST", `/sessions/${id}/messages`, { content, ...options }),
  answer: (id: string, answers: { selected: string[]; text: string | null }[]) => api<unknown>("POST", `/sessions/${id}/answers`, { answers }),
  cancel: (id: string) => api<unknown>("POST", `/sessions/${id}/cancel`),
  accessRequests: (id: string) => api<List<ApiAccessRequest>>("GET", `/sessions/${id}/access_requests`).then((list) => list.data),
  decideAccess: (id: string, request: string, approve: boolean) => api<unknown>("POST", `/sessions/${id}/access_requests/${request}/${approve ? "approve" : "reject"}`),

  sandboxFiles: (id: string) => api<List<ApiSandboxFile>>("GET", `/sessions/${id}/files`),
  moveSandboxFile: (id: string, from: string, to: string) => api<ApiSandboxFile>("POST", `/sessions/${id}/files/move`, { from, to }),
  deleteSandboxFile: (id: string, path: string) => api<unknown>("DELETE", `/sessions/${id}/files/${encodePath(path)}`),

  files: () => api<List<ApiFile>>("GET", "/library").then((list) => list.data),
  deleteFile: (path: string) => api<unknown>("DELETE", `/library/files/${path.split("/").map(encodeURIComponent).join("/")}`),
};
