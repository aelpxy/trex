import { useCallback, useEffect, useRef, useState } from "react";
import { useNavigate } from "react-router";

import { useWorkspace } from "~/components/workspace/workspace-provider";
import { ApiError, streamEvents, type StreamEvent } from "~/lib/api";
import { trex, type ApiAccessRequest, type ApiItem, type ApiQuestion, type ApiSession, type ApiUsage } from "~/lib/trex";

import { applyEvent } from "./events";
import { sessionSettings, type ChatSettings } from "./models";
import { EMPTY_USAGE, type AssistantMessage, type ChatEvent, type Message, type Part, type Response, type Usage } from "./types";

const TITLE_MAX_LENGTH = 60;
const SANDBOX_ROOT = "/sandbox/";
const ASK_USER = "ask_user";
const UPDATE_PLAN = "update_plan";
const APPLY_PATCH = "apply_patch";
const PLAN_LABEL = "Plan";
const COMPACTING_LABEL = "Summarizing context";

export type ChatData = { session: ApiSession; items: ApiItem[]; access: ApiAccessRequest[]; usage: ApiUsage[] };

function addUsage(total: Usage, entry: { input_tokens: number; cached_input_tokens: number; output_tokens: number; reasoning_tokens: number; credits: number; duration_ms: number }): Usage {
  return {
    inputTokens: total.inputTokens + entry.input_tokens,
    cachedTokens: total.cachedTokens + entry.cached_input_tokens,
    outputTokens: total.outputTokens + entry.output_tokens,
    reasoningTokens: total.reasoningTokens + entry.reasoning_tokens,
    credits: total.credits + entry.credits,
    responses: total.responses + 1,
    modelMs: total.modelMs + entry.duration_ms,
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
function describe(name: string, args: Record<string, unknown>) {
  const main = args.command ?? args.path ?? args.url ?? args.pattern ?? args.library_path ?? args.sandbox_path ?? args.id ?? args.timezone;
  return typeof main === "string" && main ? `${name} ${shortPath(main)}` : name;
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
  if (name === ASK_USER || name === UPDATE_PLAN || name === APPLY_PATCH) return [];
  if (name === "bash") return [{ type: "tool.call", id, name: "shell", input: { command: `${String(args.command ?? "")}${args.background ? " &" : ""}` } }];
  if (name === "write_file") {
    return [
      { type: "tool.call", id, name: "write_file", input: { path: shortPath(args.path) } },
      { type: "tool.output", id, delta: String(args.content ?? "") },
    ];
  }
  if (name === "edit_file") return [{ type: "tool.call", id, name: "edit_file", input: { path: shortPath(args.path), before: String(args.old_string ?? ""), after: String(args.new_string ?? "") } }];
  return [{ type: "tool.call", id, name: "shell", input: { command: describe(name, args) } }];
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
  const ok = !isError && (!exit || exit[1] === "0");
  events.push({ type: "tool.result", id, ok, summary: exit ? `exit ${exit[1]}` : isError ? "failed" : undefined });
  return events;
}

// a retried, interrupted or resumed turn is replayed from the last finished tool
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
    if (item.type === "message" && item.role === "user") messages.push({ id: crypto.randomUUID(), role: "user", content: item.text });
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
    apply([{ type: "text.delta", delta: `\n\n**The run failed.** ${session.last_error}` }]);
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

type UseChatOptions = { chatId?: string; data?: ChatData; fresh?: FreshChat; settings: ChatSettings };

export function useChat({ chatId, data, fresh, settings }: UseChatOptions) {
  const chat = data;
  const calls = useRef(new Map<string, ToolCall>());
  const [messages, setMessages] = useState<Message[]>(() => {
    if (fresh) return [{ id: crypto.randomUUID(), role: "user", content: fresh.content }, newAssistant()];
    return data ? messagesFrom(data, calls.current) : [];
  });
  const [title, setTitle] = useState<string | undefined>(data?.session.title ?? (fresh ? titleFrom(fresh.content) : undefined));
  const usage = useRef<Usage>(EMPTY_USAGE);
  // a chat reloaded mid-run replays that run from its start, rebuilt on top of the items saved before it
  const replaying = useRef(Boolean(data && !fresh && data.session.status === "running"));
  const answers = useRef(new Map<string, string>());
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
        case "text.delta":
        case "reasoning.delta":
          emit({ type: event.type, delta: String(data.delta ?? "") });
          break;
        case "tool.call": {
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
        case "plan.updated": {
          const steps = Array.isArray(data.steps) ? (data.steps as { step: string; status: string }[]) : [];
          const done = steps.filter((step) => step.status === "completed").length;
          const active = steps.find((step) => step.status === "in_progress")?.step;
          const label = `${PLAN_LABEL} · ${done} of ${steps.length} done${active ? ` · ${active}` : ""}`;
          updateLast((message) => setStatus(message, label, true, (part) => part.type === "status" && part.label.startsWith(PLAN_LABEL)));
          break;
        }
        case "context.compacting":
          updateLast((message) => setStatus(message, COMPACTING_LABEL, false));
          break;
        case "context.compacted":
          updateLast((message) => setStatus(message, "Context summarized", true, (part) => part.type === "status" && part.label === COMPACTING_LABEL));
          break;
        case "usage":
          usage.current = addUsage(usage.current, {
            input_tokens: Number(data.input_tokens ?? 0),
            cached_input_tokens: Number(data.cached_input_tokens ?? 0),
            output_tokens: Number(data.output_tokens ?? 0),
            reasoning_tokens: Number(data.reasoning_tokens ?? 0),
            credits: Number(data.credits ?? 0),
            duration_ms: Number(data.duration_ms ?? 0),
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
        case "message.received":
          usage.current = EMPTY_USAGE;
          setMessages((current) => {
            const last = current.at(-1);
            const settled: Message[] = last?.role === "assistant" ? [...current.slice(0, -1), { ...last, state: "completed", endedAt: Date.now() }] : current;
            return [...settled, { id: crypto.randomUUID(), role: "user", content: String(data.content ?? "") }, newAssistant()];
          });
          break;
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
          break;
        case "run.failed": {
          const text = data.code === "insufficient_credits" ? "**You're out of credits.** The run stopped after its last step." : `**The run failed.** ${String(data.error ?? "")}`;
          emit({ type: "text.delta", delta: `\n\n${text}` }, { type: "run.completed" });
          break;
        }
      }
    },
    [chatId, chat, emit, updateLast, renameChat],
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
    const text =
      error instanceof ApiError && error.type === "insufficient_credits_error"
        ? "**You're out of credits.** Add credits to keep chatting."
        : `**Couldn't send the message.** ${error instanceof Error ? error.message : String(error)}`;
    updateLast((message) => applyEvent(applyEvent(message, { type: "text.delta", delta: text }), { type: "run.completed" }));
  }, [updateLast]);

  const send = useCallback(
    (content: string) => {
      setMessages((current) => [...current, { id: crypto.randomUUID(), role: "user", content }, newAssistant()]);
      if (!chatId) {
        // a new chat gets its session first, then continues on its own route
        trex
          .createSession(sessionSettings(settings))
          .then(async (session) => {
            addChat(session);
            await trex.sendMessage(session.id, content);
            navigate(`/chat/${session.id}`, { state: { fresh: { content } satisfies FreshChat } });
          })
          .catch(fail);
        return;
      }
      if (!title) setTitle(titleFrom(content));
      trex.sendMessage(chatId, content).catch(fail);
    },
    [chatId, settings, title, addChat, navigate, fail],
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

  return { messages, running, send, stop, respond, title, rename };
}
