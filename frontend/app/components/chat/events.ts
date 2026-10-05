import type { AssistantMessage, ChatEvent, Part } from "./types";

function closeReasoning(parts: Part[], now: number): Part[] {
  const last = parts.at(-1);
  return last?.type === "reasoning" && !last.endedAt ? [...parts.slice(0, -1), { ...last, endedAt: now }] : parts;
}

function updatePart<T extends Part>(parts: Part[], match: (part: Part) => part is T, change: (part: T) => T): Part[] {
  return parts.map((part) => (match(part) ? change(part) : part));
}

const isTool = (id: string) => (part: Part): part is Extract<Part, { type: "tool" }> => part.type === "tool" && part.id === id;
const isAccess = (id: string) => (part: Part): part is Extract<Part, { type: "access" }> => part.type === "access" && part.id === id;
const isQuestion = (id: string) => (part: Part): part is Extract<Part, { type: "question" }> => part.type === "question" && part.id === id;

export function applyEvent(message: AssistantMessage, event: ChatEvent, now = Date.now()): AssistantMessage {
  const parts = event.type === "reasoning.delta" ? message.parts : closeReasoning(message.parts, now);
  const last = parts.at(-1);

  switch (event.type) {
    case "sandbox.creating":
      return { ...message, parts: [...parts, { type: "status", label: "Starting sandbox", done: false }] };
    case "sandbox.ready":
      return { ...message, parts: parts.map((part) => (part.type === "status" && !part.done ? { ...part, label: "Sandbox ready", done: true } : part)) };
    case "reasoning.delta":
      return last?.type === "reasoning" && !last.endedAt
        ? { ...message, parts: [...parts.slice(0, -1), { ...last, text: last.text + event.delta }] }
        : { ...message, parts: [...parts, { type: "reasoning", text: event.delta, startedAt: now }] };
    case "text.delta":
      return last?.type === "text"
        ? { ...message, parts: [...parts.slice(0, -1), { ...last, text: last.text + event.delta }] }
        : { ...message, parts: [...parts, { type: "text", text: event.delta }] };
    case "tool.call":
      return { ...message, parts: [...parts, { type: "tool", id: event.id, name: event.name, input: event.input, output: "", state: "running", startedAt: now }] };
    case "tool.output":
      return { ...message, parts: updatePart(parts, isTool(event.id), (tool) => ({ ...tool, output: tool.output + event.delta })) };
    case "tool.result":
      return { ...message, parts: updatePart(parts, isTool(event.id), (tool) => ({ ...tool, state: event.ok ? "done" : "error", summary: event.summary, endedAt: now })) };
    case "access.requested":
      return { ...message, state: "needs_input", parts: [...parts, { type: "access", id: event.id, host: event.host, binary: event.binary, state: "pending" }] };
    case "access.resolved":
      return { ...message, state: "running", parts: updatePart(parts, isAccess(event.id), (access) => ({ ...access, state: event.approved ? "approved" : "rejected" })) };
    case "question":
      return { ...message, state: "needs_input", parts: [...parts, { type: "question", id: event.id, question: event.question, options: event.options }] };
    case "question.answered":
      return { ...message, state: "running", parts: updatePart(parts, isQuestion(event.id), (question) => ({ ...question, answer: event.answer })) };
    case "usage":
      return { ...message, parts, usage: event.usage };
    case "run.completed":
      return { ...message, parts, state: "completed", endedAt: now };
    case "run.cancelled":
      return {
        ...message,
        state: "cancelled",
        endedAt: now,
        parts: parts.map((part) => (part.type === "tool" && part.state === "running" ? { ...part, state: "error", summary: "Stopped", endedAt: now } : part)),
      };
  }
}
