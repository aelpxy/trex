export type ToolName = "shell" | "write_file" | "edit_file";

export type ReasoningPart = { type: "reasoning"; text: string; startedAt: number; endedAt?: number };
export type TextPart = { type: "text"; text: string };
export type StatusPart = { type: "status"; label: string; done: boolean };
export type ToolPart = {
  type: "tool";
  id: string;
  name: ToolName;
  input: { command?: string; path?: string; before?: string; after?: string };
  output: string;
  state: "running" | "done" | "error";
  summary?: string;
  startedAt: number;
  endedAt?: number;
};
export type AccessPart = { type: "access"; id: string; host: string; binary: string; state: "pending" | "approved" | "rejected" };
export type QuestionPart = { type: "question"; id: string; question: string; options: string[]; answer?: string };

export type Part = ReasoningPart | TextPart | StatusPart | ToolPart | AccessPart | QuestionPart;

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

export type UserMessage = { id: string; role: "user"; content: string };
export type AssistantMessage = {
  id: string;
  role: "assistant";
  parts: Part[];
  state: "running" | "needs_input" | "completed" | "cancelled";
  startedAt: number;
  endedAt?: number;
  usage?: Usage;
};
export type Message = UserMessage | AssistantMessage;

// mirrors the trex SSE event names so the live stream can feed the same reducer
export type ChatEvent =
  | { type: "sandbox.creating" }
  | { type: "sandbox.ready" }
  | { type: "reasoning.delta"; delta: string }
  | { type: "text.delta"; delta: string }
  | { type: "tool.call"; id: string; name: ToolName; input: ToolPart["input"] }
  | { type: "tool.output"; id: string; delta: string }
  | { type: "tool.result"; id: string; ok: boolean; summary?: string }
  | { type: "access.requested"; id: string; host: string; binary: string }
  | { type: "access.resolved"; id: string; approved: boolean }
  | { type: "question"; id: string; question: string; options: string[] }
  | { type: "question.answered"; id: string; answer: string }
  | { type: "usage"; usage: Usage }
  | { type: "run.completed" }
  | { type: "run.cancelled" };

export type Response = { kind: "answer"; value: string } | { kind: "access"; approved: boolean };
