import type { MessageAttachment } from "~/lib/attachments";

export type ToolName = "shell" | "write_file" | "edit_file" | "read_file" | "search" | "web" | "library" | "image" | "process" | "time";

export type ReasoningPart = { type: "reasoning"; text: string; startedAt: number; endedAt?: number };
export type TextPart = { type: "text"; text: string };
export type StatusPart = { type: "status"; label: string; done: boolean };
export type ToolPart = {
  type: "tool";
  id: string;
  name: ToolName;
  input: { command?: string; path?: string; before?: string; after?: string; detail?: string };
  // overrides the tool's usual label, e.g. "Save to library"
  title?: string;
  output: string;
  state: "running" | "done" | "error";
  summary?: string;
  startedAt: number;
  endedAt?: number;
};
export type AccessPart = { type: "access"; id: string; host: string; binary: string; state: "pending" | "approved" | "rejected" };
export type QuestionPart = { type: "question"; id: string; question: string; options: string[]; answer?: string };

export type PlanStep = { step: string; status: "pending" | "in_progress" | "completed" };
export type PlanPart = { type: "plan"; explanation?: string; steps: PlanStep[] };

// `retry` when the run can be continued from where it stopped
export type ErrorPart = { type: "error"; title: string; detail?: string; retry: boolean };

export type Part = ReasoningPart | TextPart | StatusPart | ToolPart | AccessPart | QuestionPart | PlanPart | ErrorPart;

export type Usage = {
  inputTokens: number;
  cachedTokens: number;
  outputTokens: number;
  reasoningTokens: number;
  credits: number;
  responses: number;
  // time spent generating, summed over the model responses
  modelMs: number;
};

export const EMPTY_USAGE: Usage = { inputTokens: 0, cachedTokens: 0, outputTokens: 0, reasoningTokens: 0, credits: 0, responses: 0, modelMs: 0 };

export type UserMessage = { id: string; role: "user"; content: string; attachments?: MessageAttachment[] };
export type AssistantMessage = {
  id: string;
  role: "assistant";
  parts: Part[];
  state: "running" | "needs_input" | "completed" | "cancelled";
  startedAt: number;
  endedAt?: number;
  usage?: Usage;
  // what the model is writing before its tool call arrives, such as a long file
  writing?: string;
};
export type Message = UserMessage | AssistantMessage;

// mirrors the trex SSE event names so the live stream can feed the same reducer
export type ChatEvent =
  | { type: "sandbox.creating" }
  | { type: "sandbox.ready" }
  | { type: "reasoning.delta"; delta: string }
  | { type: "text.delta"; delta: string }
  | { type: "tool.writing"; label: string }
  // a tool call still being written, shown as it arrives and replaced by its tool.call
  | { type: "tool.draft"; id: string; name: ToolName; input: ToolPart["input"]; output: string }
  | { type: "tool.discard"; id: string }
  | { type: "tool.call"; id: string; name: ToolName; input: ToolPart["input"]; title?: string }
  | { type: "tool.output"; id: string; delta: string }
  | { type: "tool.result"; id: string; ok: boolean; summary?: string }
  | { type: "access.requested"; id: string; host: string; binary: string }
  | { type: "access.resolved"; id: string; approved: boolean }
  | { type: "question"; id: string; question: string; options: string[] }
  | { type: "question.answered"; id: string; answer: string }
  | { type: "plan.updated"; explanation?: string; steps: PlanStep[] }
  | { type: "usage"; usage: Usage }
  | { type: "run.completed" }
  | { type: "run.failed"; title: string; detail?: string; retry: boolean }
  | { type: "run.cancelled" };

export type Response = { kind: "answer"; value: string } | { kind: "access"; approved: boolean };
