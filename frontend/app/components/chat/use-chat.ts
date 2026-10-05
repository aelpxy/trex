import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router";

import { useWorkspace } from "~/components/workspace/workspace-provider";
import { ApiError, streamEvents, type StreamEvent } from "~/lib/api";
import { kindOf, type MessageAttachment } from "~/lib/attachments";
import { partialString } from "~/lib/partial-json";
import { trex, type ApiAccessRequest, type ApiItem, type ApiItemAttachment, type ApiQuestion, type ApiSession, type ApiUsage } from "~/lib/trex";

import { applyEvent } from "./events";
import { sessionSettings, type ChatSettings } from "./models";
import { EMPTY_USAGE, type AssistantMessage, type ChatEvent, type Message, type Part, type PlanStep, type Response, type ToolName, type ToolPart, type Usage } from "./types";

const TITLE_MAX_LENGTH = 60;
const SANDBOX_ROOT = "/sandbox/";
const ASK_USER = "ask_user";
const UPDATE_PLAN = "update_plan";
const APPLY_PATCH = "apply_patch";
const COMPACTING_LABEL = "Summarizing context";

export type ChatData = { session: ApiSession; items: ApiItem[]; access: ApiAccessRequest[]; usage: ApiUsage[] };

type UsageEntry = Omit<ApiUsage, "created_at">;

function addUsage(total: Usage, entry: UsageEntry): Usage {
  return {
    inputTokens: total.inputTokens + entry.input_tokens,
    cachedTokens: total.cachedTokens + entry.cached_input_tokens,
    cacheWriteTokens: total.cacheWriteTokens + entry.cache_write_tokens,
    outputTokens: total.outputTokens + entry.output_tokens,
    reasoningTokens: total.reasoningTokens + entry.reasoning_tokens,
    credits: total.credits + entry.credits,
    responses: total.responses + 1,
    modelMs: total.modelMs + entry.duration_ms,
    firstTokenMs: total.firstTokenMs + (entry.first_token_ms ?? 0),
    firstTokenCount: total.firstTokenCount + (entry.first_token_ms == null ? 0 : 1),
    peakInputTokens: Math.max(total.peakInputTokens, entry.input_tokens),
    models: total.models.includes(entry.model) ? total.models : [...total.models, entry.model],
  };
}

// each turn runs from its user message to the next one, so a response's usage belongs to the turn
// it finished in; the turn's last item marks when it ended
function timeTurns(messages: Message[], turns: { started: number; ended: number }[], usage: ApiUsage[], running: boolean): Message[] {
  let turn = -1;
  return messages.map((message, index) => {
    if (message.role === "user") {
      turn += 1;
      return message;
    }
    const window = turns[turn];
    if (!window) return message;
    const next = turns[turn + 1]?.started ?? Number.POSITIVE_INFINITY;
    const entries = usage.filter((entry) => entry.created_at >= window.started && entry.created_at < next);
    const last = index === messages.length - 1;
    const ended = Math.max(window.ended, ...entries.map((entry) => entry.created_at));
    return {
      ...message,
      startedAt: window.started,
      endedAt: last && running ? undefined : ended,
      usage: entries.length ? entries.reduce(addUsage, EMPTY_USAGE) : undefined,
    };
  });
}

// what a chat created on the home page carries to its own route, so the first run streams from its start
export type FreshChat = { content: string };

// a file picked in the composer, read into a data url
export type OutgoingAttachment = MessageAttachment & { url: string };

// a message sent during a run, waiting for the agent to read it
export type QueuedMessage = { id: string; content: string; attachments: MessageAttachment[] };

const savedAttachments = (item: { attachments: ApiItemAttachment[] }): MessageAttachment[] =>
  item.attachments.map((saved) => ({ id: saved.id, name: saved.filename ?? saved.mime_type, kind: kindOf(saved.mime_type) }));

// a refreshed page picks the queue back up from the server, which keeps it until the agent reads it
const queuedFrom = (data?: ChatData): QueuedMessage[] =>
  (data?.session.queued_messages ?? []).map((message) => ({ id: crypto.randomUUID(), content: message.content, attachments: savedAttachments(message) }));

type ToolCall = { name: string; args: Record<string, unknown> };

const shortPath = (path: unknown) => (typeof path === "string" && path.startsWith(SANDBOX_ROOT) ? path.slice(SANDBOX_ROOT.length) : String(path ?? ""));

function parseArgs(raw: string): Record<string, unknown> {
  try {
    return JSON.parse(raw) as Record<string, unknown>;
  } catch {
    return {};
  }
}

// tools other than bash and the file editors show as a command line: the tool and its main argument
function failedEvent(code: string | undefined, error: string): ChatEvent {
  if (code === "insufficient_credits") return { type: "run.failed", title: "Your balance ran out", detail: "The run stopped after its last step. Your balance tops up with your plan each month.", retry: false };
  return { type: "run.failed", title: "The run failed", detail: error || undefined, retry: true };
}

function planEvent(explanation: unknown, steps: unknown): ChatEvent {
  const valid = Array.isArray(steps) ? steps.filter((step): step is PlanStep => typeof step?.step === "string" && ["pending", "in_progress", "completed"].includes(step?.status)) : [];
  return { type: "plan.updated", explanation: typeof explanation === "string" && explanation ? explanation : undefined, steps: valid };
}

const text = (value: unknown) => (typeof value === "string" ? value : "");

// how the cards label each tool; anything unknown shows as a command with its main argument
function toolView(name: string, args: Record<string, unknown>): { name: ToolName; title?: string; input: ToolPart["input"] } {
  switch (name) {
    case "read_file":
      return { name: "read_file", input: { path: shortPath(args.path) } };
    case "glob":
    case "grep": {
      const where = text(args.path) ? ` in ${shortPath(args.path) || "/"}` : "";
      return { name: "search", input: { detail: `${text(args.pattern)}${where}` } };
    }
    case "web_fetch":
      return { name: "web", input: { detail: text(args.url) } };
    case "library_list":
      return { name: "library", title: "Browse library", input: { detail: text(args.prefix) } };
    case "library_load":
      return { name: "library", title: "Load from library", input: { detail: text(args.library_path) } };
    case "library_save":
      return { name: "library", title: "Save to library", input: { detail: text(args.library_path) || shortPath(args.sandbox_path) } };
    case "view_image":
      return { name: "image", input: { path: shortPath(args.path) } };
    case "process_output":
      return { name: "process", input: { detail: text(args.id) || "all processes" } };
    case "stop_process":
      return { name: "process", title: "Stop process", input: { detail: text(args.id) } };
    case "get_current_time":
      return { name: "time", input: { detail: text(args.timezone) } };
    case "show_preview":
      return { name: "web", title: "Open preview", input: { detail: `localhost:${String(args.port ?? "")}${text(args.path) || "/"}` } };
    default: {
      const main = args.command ?? args.path ?? args.url ?? args.pattern ?? args.id;
      return { name: "shell", input: { command: typeof main === "string" && main ? `${name} ${shortPath(main)}` : name } };
    }
  }
}

function titleFrom(content: string) {
  const line = content.split("\n")[0].trim();
  return line.length > TITLE_MAX_LENGTH ? `${line.slice(0, TITLE_MAX_LENGTH).trimEnd()}…` : line;
}

const newAssistant = (startedAt: number | null = Date.now()): AssistantMessage => ({
  id: crypto.randomUUID(),
  role: "assistant",
  parts: [],
  state: "running",
  startedAt: startedAt ?? 0,
});

// the events a tool call starts with; apply_patch shows its files once they change, ask_user and
// update_plan have their own events
function callEvents(id: string, call: ToolCall): ChatEvent[] {
  const { name, args } = call;
  if (name === UPDATE_PLAN) return [planEvent(args.explanation, args.plan)];
  if (name === ASK_USER || name === APPLY_PATCH) return [];
  if (name === "bash") return [{ type: "tool.call", id, name: "shell", input: { command: `${String(args.command ?? "")}${args.background ? " &" : ""}` } }];
  if (name === "write_file") {
    return [
      { type: "tool.call", id, name: "write_file", input: { path: shortPath(args.path) } },
      { type: "tool.output", id, delta: String(args.content ?? "") },
    ];
  }
  if (name === "edit_file") return [{ type: "tool.call", id, name: "edit_file", input: { path: shortPath(args.path), before: String(args.old_string ?? ""), after: String(args.new_string ?? "") } }];
  return [{ type: "tool.call", id, ...toolView(name, args) }];
}

function resultEvents(id: string, call: ToolCall | undefined, output: string, isError: boolean, live: boolean): ChatEvent[] {
  if (!call || call.name === ASK_USER || call.name === UPDATE_PLAN) return [];
  if (call.name === APPLY_PATCH) {
    // live runs show the patched files from file.changed; history and failures show the patch itself
    if (live && !isError) return [];
    return [
      { type: "tool.call", id, name: "shell", input: { command: APPLY_PATCH } },
      { type: "tool.output", id, delta: output },
      { type: "tool.result", id, ok: !isError },
    ];
  }
  const exit = output.match(/\[exit code (-?\d+)\]\s*$/);
  const events: ChatEvent[] = [];
  // bash streams its output and the editors show their content, so only the other tools add it here
  if (call.name !== "bash" && call.name !== "write_file" && call.name !== "edit_file") events.push({ type: "tool.output", id, delta: output });
  if (call.name === "bash" && !live) events.push({ type: "tool.output", id, delta: output.replace(/\n?\[exit code -?\d+\]\s*$/, "") });
  // a command that never ran, e.g. because the sandbox didn't start, streamed nothing, so its error is the output
  if (call.name === "bash" && live && isError) events.push({ type: "tool.output", id, delta: output });
  if (call.name === "show_preview" && !isError && typeof call.args.port === "number") {
    events.push({ type: "preview.opened", port: call.args.port, path: text(call.args.path) || "/" });
  }
  const ok = !isError && (!exit || exit[1] === "0");
  events.push({ type: "tool.result", id, ok, summary: exit ? `exit ${exit[1]}` : isError ? "failed" : undefined });
  return events;
}

// a retried, interrupted or resumed turn is replayed from the last finished tool
// what a tool call looks like while its arguments are still arriving; tools without a preview show the writing indicator
function draftEvent(id: string, name: string, args: string): ChatEvent | null {
  if (name === "bash") return { type: "tool.draft", id, name: "shell", input: { command: partialString(args, "command") ?? "" }, output: "" };
  if (name === "write_file") return { type: "tool.draft", id, name: "write_file", input: { path: shortPath(partialString(args, "path", true) ?? "") }, output: partialString(args, "content") ?? "" };
  if (name === "edit_file") {
    const input = { path: shortPath(partialString(args, "path", true) ?? ""), before: partialString(args, "old_string") ?? "", after: partialString(args, "new_string") ?? "" };
    return { type: "tool.draft", id, name: "edit_file", input, output: "" };
  }
  if (name === APPLY_PATCH) return { type: "tool.draft", id, name: "shell", input: { command: APPLY_PATCH }, output: partialString(args, "patch") ?? "" };
  return null;
}

// previews reparse the whole arguments, so long files redraw a few times a second rather than per token
const DRAFT_REFRESH_MS = 100;

const WRITES_FILES = new Set([APPLY_PATCH, "write_file", "edit_file"]);

const writingLabel = (name: string) => (WRITES_FILES.has(name) ? "Writing code" : name === "bash" ? "Writing a command" : "Working");

function dropUnsettled(message: AssistantMessage): AssistantMessage {
  let keep = 0;
  message.parts.forEach((part, index) => {
    if (part.type === "tool" && part.state !== "running") keep = index + 1;
  });
  return { ...message, parts: message.parts.slice(0, keep) };
}

function setStatus(message: AssistantMessage, label: string, done: boolean, replacing?: (part: Part) => boolean): AssistantMessage {
  const index = replacing ? message.parts.findIndex(replacing) : -1;
  const part: Part = { type: "status", label, done };
  if (index === -1) return { ...message, parts: [...message.parts, part] };
  return { ...message, parts: message.parts.map((existing, at) => (at === index ? part : existing)) };
}

function questionEvents(questions: ApiQuestion[]): ChatEvent[] {
  return questions.map((question, index) => ({ type: "question", id: `q${index}`, question: question.question, options: question.options.map((option) => option.label) }));
}

function accessEvent(request: ApiAccessRequest): ChatEvent {
  return { type: "access.requested", id: request.id, host: request.endpoints.join(", "), binary: request.binary };
}

// the conversation as saved; with `upto`, only the items that existed when the running run started,
// since its events are replayed to rebuild the rest
function messagesFrom(data: ChatData, calls: Map<string, ToolCall>, upto?: number): Message[] {
  const running = upto !== undefined || data.session.status === "running";
  const messages: Message[] = [];
  const turns: { started: number; ended: number }[] = [];
  const assistant = () => {
    const last = messages.at(-1);
    if (last?.role === "assistant") return last;
    const message: AssistantMessage = { ...newAssistant(0), state: "completed" };
    messages.push(message);
    return message;
  };
  const apply = (events: ChatEvent[]) => {
    const message = assistant();
    const updated = events.reduce((current, event) => applyEvent(current, event, 0), message);
    messages[messages.length - 1] = { ...updated, state: "completed" };
  };

  for (const item of upto === undefined ? data.items : data.items.filter((saved) => saved.seq <= upto)) {
    if (item.type === "message" && item.role === "user") turns.push({ started: item.created_at, ended: item.created_at });
    else if (turns.length) turns[turns.length - 1].ended = item.created_at;
    if (item.type === "message" && item.role === "user") messages.push({ id: crypto.randomUUID(), role: "user", content: item.text, attachments: savedAttachments(item) });
    else if (item.type === "message") apply([{ type: "text.delta", delta: item.text }]);
    else if (item.type === "reasoning") {
      const message = assistant();
      messages[messages.length - 1] = { ...message, parts: [...message.parts, { type: "reasoning", text: item.summary, startedAt: 0, endedAt: 0 }] };
    }
    else if (item.type === "compaction") messages[messages.length - 1] = setStatus(assistant(), "Context summarized", true);
    else if (item.type === "tool_call") {
      const call = { name: item.name, args: parseArgs(item.arguments) };
      calls.set(item.call_id, call);
      apply(callEvents(item.call_id, call));
    } else if (item.type === "tool_result") apply(resultEvents(item.call_id, calls.get(item.call_id), item.output, item.output.startsWith("error:"), false));
  }

  const { session } = data;
  if (running) messages.push(newAssistant(null));
  else if (session.status === "needs_input" && session.pending_questions) {
    const message = questionEvents(session.pending_questions).reduce((current, event) => applyEvent(current, event, 0), assistant());
    messages[messages.length - 1] = message;
  } else if (session.status === "failed" && session.last_error) {
    apply([failedEvent(undefined, session.last_error)]);
  }
  const pending = data.access.filter((request) => request.status === "pending");
  if (pending.length) {
    const message = assistant();
    messages[messages.length - 1] = { ...pending.map(accessEvent).reduce((current, event) => applyEvent(current, event, 0), message), state: message.state };
  }
  // tools have no saved timings, so they show no duration rather than a made-up one
  const untimed = messages.map((message) =>
    message.role === "assistant" ? { ...message, parts: message.parts.map((part) => (part.type === "tool" ? { ...part, endedAt: undefined } : part)) } : message,
  );
  return timeTurns(untimed, turns, data.usage, running);
}

type UseChatOptions = { chatId?: string; data?: ChatData; fresh?: FreshChat; settings: ChatSettings; projectId?: string };

export function useChat({ chatId, data, fresh, settings, projectId }: UseChatOptions) {
  const chat = data;
  const calls = useRef(new Map<string, ToolCall>());
  const drafts = useRef(new Map<string, { name: string; args: string; shownAt: number }>());
  const [messages, setMessages] = useState<Message[]>(() => {
    if (fresh) {
      // the first message is saved before a fresh chat opens, so its attachments come from there
      const first = data?.items.find((item) => item.type === "message" && item.role === "user");
      const attachments = first?.type === "message" ? savedAttachments(first) : [];
      return [{ id: crypto.randomUUID(), role: "user", content: fresh.content, attachments }, newAssistant()];
    }
    return data ? messagesFrom(data, calls.current) : [];
  });
  const [title, setTitle] = useState<string | undefined>(data?.session.title ?? (fresh ? titleFrom(fresh.content) : undefined));
  const usage = useRef<Usage>(EMPTY_USAGE);
  // a chat reloaded mid-run replays that run from its start, rebuilt on top of the items saved before it
  const replaying = useRef(Boolean(data && !fresh && data.session.status === "running"));
  const answers = useRef(new Map<string, string>());
  const [queued, setQueued] = useState<QueuedMessage[]>(() => queuedFrom(data));
  const queuedRef = useRef<QueuedMessage[]>(queued);
  const changeQueued = useCallback((change: (current: QueuedMessage[]) => QueuedMessage[]) => {
    queuedRef.current = change(queuedRef.current);
    setQueued(queuedRef.current);
  }, []);
  // a run that stops early saves the messages it never read to history, after its own output
  const flushQueued = useCallback(() => {
    const leftover = queuedRef.current;
    if (!leftover.length) return;
    changeQueued(() => []);
    setMessages((current) => [...current, ...leftover.map((message): Message => ({ id: message.id, role: "user", content: message.content, attachments: message.attachments }))]);
  }, [changeQueued]);
  const questions = useRef<ApiQuestion[]>(data?.session.pending_questions ?? []);
  const navigate = useNavigate();
  const { addChat, renameChat } = useWorkspace();

  const running = messages.at(-1)?.role === "assistant" && (messages.at(-1) as AssistantMessage).state === "running";

  // changes the newest assistant message, starting one if the run began elsewhere
  const updateLast = useCallback((change: (message: AssistantMessage) => AssistantMessage) => {
    setMessages((current) => {
      const last = current.at(-1);
      if (last?.role === "assistant") return [...current.slice(0, -1), change(last)];
      return [...current, change(newAssistant())];
    });
  }, []);

  const emit = useCallback((...events: ChatEvent[]) => updateLast((message) => events.reduce((current, event) => applyEvent(current, event), message)), [updateLast]);

  const onEvent = useCallback(
    (event: StreamEvent) => {
      const data = event.data;
      if ((event.type === "run.started" || event.type === "run.resumed") && replaying.current && chat) {
        replaying.current = false;
        usage.current = EMPTY_USAGE;
        calls.current.clear();
        setMessages(messagesFrom(chat, calls.current, Number(data.items ?? 0)));
        return;
      }
      switch (event.type) {
        case "run.started":
          usage.current = EMPTY_USAGE;
          setMessages((current) => {
            const last = current.at(-1);
            return last?.role === "assistant" && last.state === "running" ? current : [...current, newAssistant()];
          });
          break;
        case "run.resumed":
          updateLast((message) => setStatus(dropUnsettled({ ...message, state: "running" }), "Resumed after a restart", true));
          break;
        case "model.retrying":
          updateLast((message) => setStatus(dropUnsettled(message), `Retrying (${Number(data.attempt) + 1}/${Number(data.max_attempts)})`, true));
          break;
        case "run.interrupted":
          updateLast(dropUnsettled);
          break;
        case "sandbox.creating":
        case "sandbox.starting":
          emit({ type: "sandbox.creating" });
          break;
        case "sandbox.ready":
          emit({ type: "sandbox.ready" });
          break;
        case "sandbox.replaced":
          updateLast((message) => setStatus(message, `${String(data.reason ?? "The sandbox stopped working")}, so it was replaced with a new one. Files from before are gone.`, true));
          break;
        case "text.delta":
        case "reasoning.delta":
          emit({ type: event.type, delta: String(data.delta ?? "") });
          break;
        case "tool.call.started": {
          const draft = { name: String(data.name), args: "", shownAt: Date.now() };
          drafts.current.set(String(data.call_id), draft);
          emit(draftEvent(String(data.call_id), draft.name, "") ?? { type: "tool.writing", label: writingLabel(draft.name) });
          break;
        }
        case "tool.call.delta": {
          const draft = drafts.current.get(String(data.call_id));
          if (!draft) break;
          draft.args += String(data.delta ?? "");
          if (Date.now() - draft.shownAt < DRAFT_REFRESH_MS) break;
          draft.shownAt = Date.now();
          const preview = draftEvent(String(data.call_id), draft.name, draft.args);
          if (preview) emit(preview);
          break;
        }
        case "tool.call": {
          drafts.current.delete(String(data.call_id));
          // apply_patch's files appear one by one from file.changed, so its draft goes
          if (data.name === APPLY_PATCH) emit({ type: "tool.discard", id: String(data.call_id) });
          const call = { name: String(data.name), args: parseArgs(String(data.arguments ?? "")) };
          calls.current.set(String(data.call_id), call);
          emit(...callEvents(String(data.call_id), call));
          break;
        }
        case "tool.output":
          if (calls.current.get(String(data.call_id))?.name === "bash") emit({ type: "tool.output", id: String(data.call_id), delta: String(data.chunk ?? "") });
          break;
        case "tool.result":
          emit(...resultEvents(String(data.call_id), calls.current.get(String(data.call_id)), String(data.output ?? ""), data.is_error === true, true));
          break;
        case "file.changed": {
          const callId = String(data.call_id);
          const path = shortPath(data.path);
          const before = typeof data.before === "string" ? data.before : "";
          const after = typeof data.after === "string" ? data.after : "";
          const call = calls.current.get(callId);
          if (call?.name === APPLY_PATCH) {
            const id = `${callId}:${path}`;
            emit({ type: "tool.call", id, name: "edit_file", input: { path, before, after } }, { type: "tool.result", id, ok: true });
          } else if (call?.name === "edit_file" && typeof data.before === "string" && typeof data.after === "string") {
            // the whole file replaces the edited snippet, so the diff and the files panel show the real file
            updateLast((message) => ({ ...message, parts: message.parts.map((part) => (part.type === "tool" && part.id === callId ? { ...part, input: { ...part.input, before, after } } : part)) }));
          }
          break;
        }
        case "plan.updated":
          emit(planEvent(data.explanation, data.steps));
          break;
        case "preview.opened":
          emit({ type: "preview.opened", port: Number(data.port), path: String(data.path ?? "/") });
          break;
        case "context.compacting":
          updateLast((message) => setStatus(message, COMPACTING_LABEL, false));
          break;
        case "context.compacted":
          updateLast((message) => setStatus(message, "Context summarized", true, (part) => part.type === "status" && part.label === COMPACTING_LABEL));
          break;
        case "usage":
          usage.current = addUsage(usage.current, {
            model: String(data.model ?? ""),
            input_tokens: Number(data.input_tokens ?? 0),
            cached_input_tokens: Number(data.cached_input_tokens ?? 0),
            cache_write_tokens: Number(data.cache_write_tokens ?? 0),
            output_tokens: Number(data.output_tokens ?? 0),
            reasoning_tokens: Number(data.reasoning_tokens ?? 0),
            credits: Number(data.credits ?? 0),
            duration_ms: Number(data.duration_ms ?? 0),
            first_token_ms: typeof data.time_to_first_token_ms === "number" ? data.time_to_first_token_ms : null,
          });
          emit({ type: "usage", usage: usage.current });
          break;
        case "access.requested":
          // denials can be reported after the run finished, which must not reopen it
          updateLast((message) => {
            if (message.parts.some((part) => part.type === "access" && part.id === data.id)) return message;
            const next = applyEvent(message, accessEvent(data as unknown as ApiAccessRequest));
            return message.state === "running" ? { ...next, state: "running" } : { ...next, state: message.state };
          });
          break;
        case "question":
          questions.current = (data.questions as ApiQuestion[]) ?? [];
          answers.current.clear();
          emit(...questionEvents(questions.current));
          break;
        case "message.received": {
          usage.current = EMPTY_USAGE;
          const content = String(data.content ?? "");
          // the agent reads queued messages in order, so this is the oldest one with this text
          const sent = queuedRef.current.find((message) => message.content === content);
          if (sent) changeQueued((current) => current.filter((message) => message !== sent));
          setMessages((current) => {
            const last = current.at(-1);
            const settled: Message[] = last?.role === "assistant" ? [...current.slice(0, -1), { ...last, state: "completed", endedAt: Date.now() }] : current;
            return [...settled, { id: crypto.randomUUID(), role: "user", content, attachments: sent?.attachments }, newAssistant()];
          });
          break;
        }
        case "session.updated":
          if (typeof data.title === "string") {
            setTitle(data.title);
            if (chatId) renameChat(chatId, data.title);
          }
          break;
        case "run.completed":
          emit({ type: "run.completed" });
          break;
        case "run.cancelled":
          emit({ type: "run.cancelled" });
          flushQueued();
          break;
        case "run.failed": {
          emit(failedEvent(typeof data.code === "string" ? data.code : undefined, String(data.error ?? "")));
          flushQueued();
          break;
        }
      }
    },
    [chatId, chat, emit, updateLast, renameChat, changeQueued, flushQueued],
  );

  // one stream per open chat; a fresh chat replays its first run from the start
  useEffect(() => {
    if (!chatId) return;
    const abort = new AbortController();
    const from = fresh ? "start" : replaying.current ? "run" : "tail";
    streamEvents(chatId, { from, signal: abort.signal, onEvent }).catch((error) => console.warn("event stream closed", error));
    return () => abort.abort();
  }, [chatId, fresh, onEvent]);

  const fail = useCallback((error: unknown) => {
    const event: ChatEvent =
      error instanceof ApiError && error.type === "insufficient_credits_error"
        ? { type: "run.failed", title: "Your balance ran out", detail: "It tops up with your plan each month, or add funds.", retry: false }
        : { type: "run.failed", title: "Couldn't send the message", detail: error instanceof Error ? error.message : String(error), retry: false };
    updateLast((message) => applyEvent(message, event));
  }, [updateLast]);

  // edits and regenerations continue in a new chat, since a chat's history is never rewritten
  const branch = useCallback(
    async (message: number, content?: string) => {
      if (!chatId) return;
      const session = await trex.branch(chatId, content === undefined ? { message } : { message, content });
      addChat(session);
      navigate(`/chat/${session.id}`);
    },
    [chatId, addChat, navigate],
  );

  // continues the failed run in the same reply
  const retry = useCallback(() => {
    if (!chatId) return;
    updateLast((message) => ({ ...message, state: "running", endedAt: undefined, parts: message.parts.filter((part) => part.type !== "error") }));
    trex.retry(chatId).catch(fail);
  }, [chatId, updateLast, fail]);

  const send = useCallback(
    (content: string, attachments: OutgoingAttachment[] = [], interrupt = false) => {
      const body = { attachments: attachments.map((file) => ({ data: file.url, filename: file.name })), interrupt };
      const user: Message = { id: crypto.randomUUID(), role: "user", content, attachments };
      if (chatId && running) {
        // the agent takes it before its next step, or right away with interrupt; message.received shows it then
        const message: QueuedMessage = { id: user.id, content, attachments };
        changeQueued((current) => [...current, message]);
        trex
          .sendMessage(chatId, content, body)
          .then(({ queued: waiting }) => {
            if (waiting || !queuedRef.current.includes(message)) return;
            // the run ended first, so the message started a new one instead
            changeQueued((current) => current.filter((existing) => existing !== message));
            setMessages((current) => {
              const last = current.at(-1);
              const fresh = last?.role === "assistant" && last.state === "running" && last.parts.length === 0;
              return fresh ? [...current.slice(0, -1), user, last] : [...current, user, newAssistant()];
            });
          })
          .catch((error) => {
            changeQueued((current) => current.filter((existing) => existing !== message));
            fail(error);
          });
        return;
      }
      setMessages((current) => [...current, user, newAssistant()]);
      if (!chatId) {
        // a new chat gets its session first, then continues on its own route
        trex
          .createSession({ ...sessionSettings(settings), project_id: projectId ?? null })
          .then(async (session) => {
            addChat(session);
            await trex.sendMessage(session.id, content, body);
            navigate(`/chat/${session.id}`, { state: { fresh: { content } satisfies FreshChat } });
          })
          .catch(fail);
        return;
      }
      if (!title) setTitle(titleFrom(content));
      trex.sendMessage(chatId, content, body).catch(fail);
    },
    [chatId, running, settings, projectId, title, addChat, navigate, fail, changeQueued],
  );

  const stop = useCallback(() => {
    if (chatId) trex.cancel(chatId).catch((error) => console.warn("could not stop the run", error));
  }, [chatId]);

  const respond = useCallback(
    (key: string, response: Response) => {
      if (!chatId) return;
      if (response.kind === "access") {
        // deciding never changes whether the run is going
        updateLast((message) => ({ ...applyEvent(message, { type: "access.resolved", id: key, approved: response.approved }), state: message.state }));
        trex.decideAccess(chatId, key, response.approved).catch((error) => console.warn("could not decide the access request", error));
        return;
      }
      answers.current.set(key, response.value);
      emit({ type: "question.answered", id: key, answer: response.value });
      if (answers.current.size < questions.current.length) return updateLast((message) => ({ ...message, state: "needs_input" }));
      const body = questions.current.map((question, index) => {
        const value = answers.current.get(`q${index}`) ?? "";
        const picked = question.options.some((option) => option.label === value);
        return { selected: picked ? [value] : [], text: picked ? null : value };
      });
      answers.current.clear();
      trex.answer(chatId, body).catch(fail);
    },
    [chatId, emit, updateLast, fail],
  );

  const rename = useCallback(
    (next: string) => {
      setTitle(next);
      if (!chatId) return;
      renameChat(chatId, next);
      trex.updateSession(chatId, { title: next }).catch((error) => console.warn("could not rename the chat", error));
    },
    [chatId, renameChat],
  );

  return { messages, running, queued, send, stop, retry, branch, respond, title, rename };
}
