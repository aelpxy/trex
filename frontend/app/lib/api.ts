// the trex http api; in development vite proxies /api to the trex server, so requests stay same-origin
const BASE = import.meta.env.VITE_TREX_API ?? "/api";
const TOKEN_KEY = "trex-token";
const WORKSPACE_KEY = "trex-ui";
const RECONNECT_MS = 1000;

export class ApiError extends Error {
  constructor(
    public status: number,
    public type: string,
    message: string,
    public param: string | null = null,
  ) {
    super(message);
  }
}

export function getToken(): string | null {
  try {
    return localStorage.getItem(TOKEN_KEY);
  } catch {
    return null;
  }
}

export function setToken(token: string | null) {
  try {
    if (token) localStorage.setItem(TOKEN_KEY, token);
    else localStorage.removeItem(TOKEN_KEY);
  } catch (error) {
    console.warn("could not save the session token", error);
  }
}

// the workspace every request acts in; set by the workspace provider, starting from the saved ui state
let currentWorkspace: string | null = null;

function workspaceId(): string | null {
  if (currentWorkspace) return currentWorkspace;
  try {
    const saved = JSON.parse(localStorage.getItem(WORKSPACE_KEY) ?? "null");
    return typeof saved?.workspaceId === "string" ? saved.workspaceId : null;
  } catch {
    return null;
  }
}

export function setWorkspaceId(id: string | null) {
  currentWorkspace = id;
}

function headers(extra: Record<string, string> = {}) {
  const result: Record<string, string> = { ...extra };
  const token = getToken();
  if (token) result.authorization = `Bearer ${token}`;
  const workspace = workspaceId();
  if (workspace?.startsWith("ws_")) result["trex-workspace"] = workspace;
  return result;
}

// an expired token sends the user back to sign in instead of failing every request
function signOutIfUnauthorized(status: number) {
  if (status !== 401 || !getToken()) return;
  setToken(null);
  if (!window.location.pathname.startsWith("/auth")) window.location.assign("/auth");
}

async function failure(response: Response): Promise<never> {
  const body = await response.json().catch(() => null);
  signOutIfUnauthorized(response.status);
  const error = body?.error;
  throw new ApiError(response.status, error?.type ?? "api_error", error?.message ?? `request failed with ${response.status}`, error?.param ?? null);
}

export async function api<T>(method: string, path: string, body?: unknown): Promise<T> {
  const response = await fetch(`${BASE}/v1${path}`, {
    method,
    headers: headers(body === undefined ? {} : { "content-type": "application/json" }),
    body: body === undefined ? undefined : JSON.stringify(body),
  });
  if (!response.ok) return failure(response);
  if (response.status === 204) return undefined as T;
  return (await response.json()) as T;
}

export async function upload(path: string, file: Blob): Promise<void> {
  const response = await fetch(`${BASE}/v1/library/files/${encodePath(path)}`, {
    method: "PUT",
    headers: headers({ "content-type": "application/octet-stream" }),
    body: file,
  });
  if (!response.ok) await failure(response);
}

export async function download(path: string): Promise<Blob> {
  const response = await fetch(`${BASE}/v1/library/files/${encodePath(path)}`, { headers: headers() });
  if (!response.ok) await failure(response);
  return response.blob();
}

export const encodePath = (path: string) => path.split("/").map(encodeURIComponent).join("/");

export type StreamEvent = { id: string; type: string; data: Record<string, unknown> };

// EventSource can't send the bearer token, so the session's event stream is read with fetch;
// it reconnects with Last-Event-ID until the signal aborts
// `start` replays every event, `run` the latest run from its start, `tail` only new events
export async function streamEvents(sessionId: string, options: { from: "start" | "run" | "tail"; signal: AbortSignal; onEvent: (event: StreamEvent) => void }) {
  let lastId: string | null = null;
  while (!options.signal.aborted) {
    try {
      const query = lastId === null && options.from !== "tail" ? `?from=${options.from}` : "";
      const response = await fetch(`${BASE}/v1/sessions/${sessionId}/events${query}`, {
        headers: headers(lastId === null ? { accept: "text/event-stream" } : { accept: "text/event-stream", "last-event-id": lastId }),
        signal: options.signal,
      });
      if (!response.ok || !response.body) {
        if (response.status === 401 || response.status === 404) return failure(response);
        throw new Error(`event stream returned ${response.status}`);
      }
      const reader = response.body.pipeThrough(new TextDecoderStream()).getReader();
      let buffer = "";
      for (;;) {
        const { value, done } = await reader.read();
        if (done) break;
        buffer += value;
        let boundary: number;
        while ((boundary = buffer.indexOf("\n\n")) !== -1) {
          const block = buffer.slice(0, boundary);
          buffer = buffer.slice(boundary + 2);
          const event = parseEvent(block);
          if (!event) continue;
          lastId = event.id || lastId;
          options.onEvent(event);
        }
      }
    } catch (error) {
      if (options.signal.aborted) return;
      if (error instanceof ApiError) throw error;
      console.warn("event stream dropped, reconnecting", error);
    }
    await new Promise((resolve) => setTimeout(resolve, RECONNECT_MS));
  }
}

function parseEvent(block: string): StreamEvent | null {
  let id = "";
  let type = "";
  const data: string[] = [];
  for (const line of block.split("\n")) {
    if (line.startsWith(":")) continue;
    const colon = line.indexOf(":");
    const field = colon === -1 ? line : line.slice(0, colon);
    const value = colon === -1 ? "" : line.slice(colon + 1).replace(/^ /, "");
    if (field === "id") id = value;
    else if (field === "event") type = value;
    else if (field === "data") data.push(value);
  }
  if (data.length === 0) return null;
  try {
    const payload = JSON.parse(data.join("\n")) as Record<string, unknown>;
    return { id, type: type || String(payload.type ?? ""), data: payload };
  } catch (error) {
    console.warn("unreadable event", error);
    return null;
  }
}
