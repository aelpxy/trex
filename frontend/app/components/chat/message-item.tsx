import { Markdown } from "~/components/markdown/markdown";

import { AccessRequest } from "./parts/access-request";
import { QuestionCard } from "./parts/question-card";
import { ReasoningBlock } from "./parts/reasoning-block";
import { RunFooter } from "./parts/run-footer";
import { RunStatus } from "./parts/run-status";
import { ToolCall } from "./parts/tool-call";
import { Thinking } from "./thinking";
import type { AssistantMessage, Message, Part, Response } from "./types";

type Respond = (key: string, response: Response) => void;

function isBusy(part: Part | undefined) {
  if (!part) return false;
  if (part.type === "text") return true;
  if (part.type === "reasoning") return part.endedAt === undefined;
  if (part.type === "tool") return part.state === "running";
  if (part.type === "status") return !part.done;
  return false;
}

function PartView({ part, onRespond }: { part: Part; onRespond: Respond }) {
  switch (part.type) {
    case "text":
      return <Markdown>{part.text}</Markdown>;
    case "reasoning":
      return <ReasoningBlock part={part} />;
    case "status":
      return <RunStatus part={part} />;
    case "tool":
      return <ToolCall part={part} />;
    case "access":
      return <AccessRequest part={part} onResolve={(approved) => onRespond(part.id, { kind: "access", approved })} />;
    case "question":
      return <QuestionCard part={part} onAnswer={(value) => onRespond(part.id, { kind: "answer", value })} />;
  }
}

function AssistantMessageView({ message, onRespond }: { message: AssistantMessage; onRespond: Respond }) {
  const showThinking = message.state === "running" && !isBusy(message.parts.at(-1));

  return (
    <div className="space-y-3">
      {message.parts.map((part, index) => (
        <PartView key={"id" in part ? part.id : `${part.type}-${index}`} part={part} onRespond={onRespond} />
      ))}
      {showThinking && <Thinking />}
      <RunFooter message={message} />
    </div>
  );
}

export function MessageItem({ message, onRespond }: { message: Message; onRespond: Respond }) {
  if (message.role === "user") {
    return (
      <div className="flex justify-end">
        <p className="max-w-[85%] rounded-2xl bg-subtle px-4 py-2.5 text-sm leading-6 break-words whitespace-pre-wrap">{message.content}</p>
      </div>
    );
  }

  return <AssistantMessageView message={message} onRespond={onRespond} />;
}
