import { api, ApiError, encodePath } from "./api";

// shapes of the trex api, see /docs on the trex server

export type ApiUser = { id: string; email: string; name: string; role: "user" | "admin"; created_at: number };
export type ApiWorkspace = { id: string; name: string; plan: string; role: string; credits: number; created_at: number };
export type ApiScheduledTask = {
  id: string;
  title: string;
  prompt: string;
  model: string;
  reasoning_effort: string | null;
  schedule: string;
  timezone: string;
  paused: boolean;
  project_id: string | null;
  next_run_at: number | null;
  last_run_at: number | null;
  last_error: string | null;
  created_at: number;
  updated_at: number;
};

export type TaskInput = {
  title: string;
  prompt: string;
  model: string;
  reasoning_effort: string | null;
  schedule: string;
  timezone: string;
  project_id: string | null;
  paused?: boolean;
};

export type ApiSignInSession = {
  id: string;
  ip: string | null;
  last_ip: string | null;
  user_agent: string | null;
  current: boolean;
  created_at: number;
  last_used_at: number;
  expires_at: number;
};
export type ApiMe = { user: ApiUser; workspaces: ApiWorkspace[] };
export type List<T> = { object: "list"; data: T[]; has_more: boolean };
export type Paged<T> = List<T> & { total_count: number };

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
  queued_messages: { content: string; attachments: ApiItemAttachment[] }[];
  created_at: number;
  updated_at: number;
};

export type ApiProject = { id: string; name: string; instructions: string | null; created_at: number; updated_at: number };

export type ApiItemAttachment = { id: string; kind: string; mime_type: string; filename: string | null };

export type ApiItem = (
  | { type: "message"; role: string; text: string; attachments: ApiItemAttachment[] }
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
  cache_write_tokens: number;
  output_tokens: number;
  reasoning_tokens: number;
  credits: number;
  duration_ms: number;
  first_token_ms: number | null;
};

export type ApiAttachmentInput = { data: string; filename: string };

export type ApiAccessRequest = { id: string; status: string; endpoints: string[]; binary: string; rationale: string };

export type ApiSandboxFile = { path: string; size: number; modified_at: number };

export type ApiFile = { path: string; size: number; modified_at: number };

export type ApiAdminWorkspace = { id: string; name: string; plan: string; credits: number; owner_email: string | null; allowed_models: string[] | null; created_at: number };
export type ApiAdminUser = { id: string; email: string; name: string; role: "user" | "admin"; workspaces: number; credits: number | null; created_at: number; last_active_at: number | null; suspended_at: number | null };
export type ApiModelUsage = { model: string; credits: number; input_tokens: number; output_tokens: number; responses: number };
export type ApiOverview = {
  users: number;
  admins: number;
  workspaces: number;
  chats: number;
  running: number;
  spend_today: number;
  tokens_today: number;
  spend_month: number;
  tokens_month: number;
  top_models: ApiModelUsage[];
};
export type ApiUsageReport = {
  days: number;
  daily: { day: number; credits: number; input_tokens: number; output_tokens: number; responses: number }[];
  models: ApiModelUsage[];
  accounts: { user: string; name: string; email: string; credits: number; tokens: number; responses: number }[];
};
export type ApiLiveRun = {
  session: string;
  title: string | null;
  workspace: string;
  workspace_name: string;
  owner_email: string | null;
  model: string;
  reasoning_effort: string | null;
  fast: boolean;
  scheduled: boolean;
  started_at: number | null;
  heartbeat_at: number | null;
  cancel_requested: boolean;
  credits: number;
  responses: number;
};
export type ApiLogLine = { seq: number; time: number; level: "error" | "warn" | "info" | "debug" | "trace"; target: string; message: string; fields: string };

export type ApiPlan = { id: string; name: string; monthly_credits: number };
export type ApiLedgerEntry = { id: string; amount: number; balance: number; kind: "grant" | "usage" | "adjustment"; description: string; created_at: number };

export type ApiMonthUsage = {
  period_start: number;
  credits: number;
  input_tokens: number;
  output_tokens: number;
  responses: number;
  models: { model: string; name: string; credits: number; input_tokens: number; output_tokens: number; responses: number }[];
};
export type ApiCredits = { balance: number; plan: { id: string; name: string; monthly_credits: number } | null };

const MAX_PAGE = 100;

const pageQuery = (page: number, perPage: number, search: string, sort = "") =>
  new URLSearchParams({ page: String(page), limit: String(perPage), ...(search.trim() ? { q: search.trim() } : {}), ...(sort ? { sort } : {}) }).toString();

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

// the signed-in account, or null when the session cookie is missing or expired
export async function signedIn(): Promise<ApiMe | null> {
  try {
    return await trex.me();
  } catch (error) {
    if (error instanceof ApiError && error.status === 401) return null;
    throw error;
  }
}

export const trex = {
  signup: (body: { name: string; email: string; password: string }) => api<ApiMe>("POST", "/auth/signup", body),
  login: (body: { email: string; password: string }) => api<ApiMe>("POST", "/auth/login", body),
  signInSessions: () => api<List<ApiSignInSession>>("GET", "/me/sessions").then((list) => list.data),
  endSignInSession: (id: string) => api<void>("DELETE", `/me/sessions/${id}`),
  signOutOthers: () => api<void>("POST", "/me/sessions/sign_out_others"),
  logout: () => api<void>("POST", "/auth/logout"),
  me: () => api<ApiMe>("GET", "/me"),
  models: () => api<List<ApiModel>>("GET", "/models").then((list) => list.data),
  credits: () => api<ApiCredits>("GET", "/credits"),
  monthUsage: () => api<ApiMonthUsage>("GET", "/credits/usage"),
  admin: {
    workspaces: (page: number, perPage: number, search: string, sort = "") => api<Paged<ApiAdminWorkspace>>("GET", `/admin/workspaces?${pageQuery(page, perPage, search, sort)}`),
    adjustCredits: (workspace: string, body: { amount: number; description: string }) =>
      api<{ workspace: string; balance: number }>("POST", `/admin/workspaces/${workspace}/credits`, body),
    setPlan: (workspace: string, plan: string) => api<void>("POST", `/admin/workspaces/${workspace}/plan`, { plan }),
    setModels: (workspace: string, models: string[] | null) => api<void>("PATCH", `/admin/workspaces/${workspace}`, { allowed_models: models }),
    models: () => api<List<ApiModel>>("GET", "/admin/models").then((list) => list.data),
    overview: () => api<ApiOverview>("GET", "/admin/overview"),
    usage: (days: number) => api<ApiUsageReport>("GET", `/admin/usage?days=${days}`),
    runs: () => api<List<ApiLiveRun>>("GET", "/admin/runs").then((list) => list.data),
    cancelRun: (session: string) => api<void>("POST", `/admin/runs/${session}/cancel`),
    users: (page: number, perPage: number, search: string, sort = "") => api<Paged<ApiAdminUser>>("GET", `/admin/users?${pageQuery(page, perPage, search, sort)}`),
    updateUser: (user: string, changes: { role?: "user" | "admin"; suspended?: boolean }) => api<void>("PATCH", `/admin/users/${user}`, changes),
    setPassword: (user: string, password: string) => api<void>("POST", `/admin/users/${user}/password`, { password }),
    signOut: (user: string) => api<void>("POST", `/admin/users/${user}/sign_out`),
    deleteUser: (user: string) => api<void>("DELETE", `/admin/users/${user}`),
    signInSessions: (user: string) => api<List<ApiSignInSession>>("GET", `/admin/users/${user}/sessions`).then((list) => list.data),
    endSignInSession: (user: string, session: string) => api<void>("DELETE", `/admin/users/${user}/sessions/${session}`),
    library: (workspace: string) => api<List<ApiFile>>("GET", `/admin/workspaces/${workspace}/library`).then((list) => list.data),
    logs: () => api<List<ApiLogLine>>("GET", "/admin/logs?limit=1000").then((list) => list.data),
  },
  plans: () => api<List<ApiPlan>>("GET", "/plans").then((list) => list.data),
  ledger: (page: number, perPage: number) => api<Paged<ApiLedgerEntry>>("GET", `/credits/ledger?page=${page}&limit=${perPage}`),
  updateMe: (body: { name: string }) => api<ApiUser>("PATCH", "/me", body),
  changePassword: (body: { current_password: string; new_password: string }) => api<void>("POST", "/me/password", body),

  projects: () => all<ApiProject>("/projects"),
  createProject: (name: string) => api<ApiProject>("POST", "/projects", { name }),
  updateProject: (id: string, body: { name?: string; instructions?: string | null }) => api<ApiProject>("PATCH", `/projects/${id}`, body),
  deleteProject: (id: string) => api<unknown>("DELETE", `/projects/${id}`),

  sessions: () => all<ApiSession>("/sessions"),
  scheduled: {
    list: () => api<List<ApiScheduledTask>>("GET", "/scheduled_tasks").then((list) => list.data),
    get: (id: string) => api<ApiScheduledTask>("GET", `/scheduled_tasks/${id}`),
    create: (body: TaskInput) => api<ApiScheduledTask>("POST", "/scheduled_tasks", body),
    update: (id: string, body: Partial<TaskInput>) => api<ApiScheduledTask>("PATCH", `/scheduled_tasks/${id}`, body),
    remove: (id: string) => api<void>("DELETE", `/scheduled_tasks/${id}`),
    run: (id: string) => api<ApiSession>("POST", `/scheduled_tasks/${id}/run`),
    runs: (id: string) => all<ApiSession>(`/sessions?scheduled_task_id=${id}`),
  },
  session: (id: string) => api<ApiSession>("GET", `/sessions/${id}`),
  createSession: (body: { model: string; reasoning_effort: string | null; fast: boolean; project_id?: string | null }) => api<ApiSession>("POST", "/sessions", body),
  updateSession: (id: string, body: { title?: string; project_id?: string | null; model?: string; reasoning_effort?: string | null; fast?: boolean }) => api<ApiSession>("PATCH", `/sessions/${id}`, body),
  deleteSession: (id: string) => api<unknown>("DELETE", `/sessions/${id}`),
  items: (id: string) => api<List<ApiItem>>("GET", `/sessions/${id}/items`).then((list) => list.data),
  usage: (id: string) => api<List<ApiUsage>>("GET", `/sessions/${id}/usage`).then((list) => list.data),
  sendMessage: (id: string, content: string, options: { interrupt?: boolean; attachments?: ApiAttachmentInput[] } = {}) =>
    api<{ queued: boolean }>("POST", `/sessions/${id}/messages`, { content, ...options }),
  answer: (id: string, answers: { selected: string[]; text: string | null }[]) => api<unknown>("POST", `/sessions/${id}/answers`, { answers }),
  branch: (id: string, body: { message: number; content?: string }) => api<ApiSession>("POST", `/sessions/${id}/branch`, body),
  retry: (id: string) => api<unknown>("POST", `/sessions/${id}/retry`),
  cancel: (id: string) => api<unknown>("POST", `/sessions/${id}/cancel`),
  accessRequests: (id: string) => api<List<ApiAccessRequest>>("GET", `/sessions/${id}/access_requests`).then((list) => list.data),
  decideAccess: (id: string, request: string, approve: boolean) => api<unknown>("POST", `/sessions/${id}/access_requests/${request}/${approve ? "approve" : "reject"}`),

  createPreview: (id: string, port: number) => api<{ port: number; url: string; expires_at: number }>("POST", `/sessions/${id}/previews`, { port }),
  sandboxFiles: (id: string) => api<List<ApiSandboxFile>>("GET", `/sessions/${id}/files`),
  moveSandboxFile: (id: string, from: string, to: string) => api<ApiSandboxFile>("POST", `/sessions/${id}/files/move`, { from, to }),
  deleteSandboxFile: (id: string, path: string) => api<unknown>("DELETE", `/sessions/${id}/files/${encodePath(path)}`),

  files: () => api<List<ApiFile>>("GET", "/library").then((list) => list.data),
  moveFile: (from: string, to: string) => api<ApiFile>("POST", "/library/move", { from, to }),
  deleteFile: (path: string) => api<unknown>("DELETE", `/library/files/${path.split("/").map(encodeURIComponent).join("/")}`),
};
