import { useState } from "react";
import { Button } from "@base-ui/react/button";
import { LuPencil, LuRefreshCw } from "react-icons/lu";

import { Markdown } from "~/components/markdown/markdown";
import { iconButton } from "~/components/ui/styles";

import { libraryPath } from "~/lib/library-links";
import { previewOf } from "~/lib/preview";

import { AccessRequest } from "./parts/access-request";
import { ArtifactCard } from "./parts/artifact-card";
import { CopyButton } from "./parts/copy-button";
import { ErrorCard } from "./parts/error-card";
import { MessageEditor } from "./parts/message-editor";
import { MessageAttachments } from "./parts/message-attachments";
import { PlanCard } from "./parts/plan-card";
import { PreviewCard } from "./parts/preview-card";
import { QuestionCard } from "./parts/question-card";
import { ReasoningBlock } from "./parts/reasoning-block";
import { RunFooter } from "./parts/run-footer";
import { RunStatus } from "./parts/run-status";
import { StepGroup } from "./parts/step-group";
import { ToolCall } from "./parts/tool-call";
import { Thinking } from "./thinking";
import type { AssistantMessage, Message, Part, Response, UserMessage } from "./types";

type Respond = (key: string, response: Response) => void;

function isBusy(part: Part | undefined) {
  if (!part) return false;
  if (part.type === "text") return true;
  if (part.type === "reasoning") return part.endedAt === undefined;
  if (part.type === "tool") return part.state === "running";
  if (part.type === "status") return !part.done;
  return false;
}

function PartView({ part, onRespond, onRetry }: { part: Part; onRespond: Respond; onRetry?: () => void }) {
  switch (part.type) {
    case "text":
      return <Markdown>{part.text}</Markdown>;
    case "reasoning":
      return <ReasoningBlock part={part} />;
    case "status":
      return <RunStatus part={part} />;
    case "tool":
      return <ToolCall part={part} />;
    case "plan":
      return <PlanCard part={part} />;
    case "error":
      return <ErrorCard part={part} onRetry={onRetry} />;
    case "preview":
      return <PreviewCard part={part} />;
    case "access":
      return <AccessRequest part={part} onResolve={(approved) => onRespond(part.id, { kind: "access", approved })} />;
    case "question":
      return <QuestionCard part={part} onAnswer={(value) => onRespond(part.id, { kind: "answer", value })} onSkip={() => onRespond(part.id, { kind: "skip" })} />;
  }
}

const MARKDOWN_LINK = /(?<!!)\[([^\]]+)\]\(([^)\s]+)\)/g;

// previewable library files the reply links to, each shown once as a card
function artifactsOf(message: AssistantMessage) {
  const artifacts = new Map<string, string>();
  for (const part of message.parts) {
    if (part.type !== "text") continue;
    for (const [, title, href] of part.text.matchAll(MARKDOWN_LINK)) {
      const path = libraryPath(href);
      if (path && previewOf(path) && !artifacts.has(path)) artifacts.set(path, title);
    }
  }
  return [...artifacts].map(([path, title]) => ({ path, title }));
}

const isStep = (part: Part) => part.type === "tool" || part.type === "reasoning" || part.type === "status";

type Block = { kind: "part"; part: Part; index: number } | { kind: "steps"; parts: Part[]; first: number };

// consecutive steps fold into one summary; replies, questions and access requests stay in view
function blocksOf(parts: Part[]): Block[] {
  const blocks: Block[] = [];
  parts.forEach((part, index) => {
    const last = blocks.at(-1);
    if (!isStep(part)) blocks.push({ kind: "part", part, index });
    else if (last?.kind === "steps") last.parts.push(part);
    else blocks.push({ kind: "steps", parts: [part], first: index });
  });
  return blocks;
}

const partKey = (part: Part, index: number) => ("id" in part ? part.id : `${part.type}-${index}`);

const replyText = (message: AssistantMessage) =>
  message.parts
    .flatMap((part) => (part.type === "text" ? [part.text.trim()] : []))
    .filter(Boolean)
    .join("\n\n");

type MessageProps = { onRespond: Respond; onRetry?: () => void; onRegenerate?: () => Promise<void>; onEdit?: (content: string) => Promise<void> };

function RegenerateButton({ onRegenerate }: { onRegenerate: () => Promise<void> }) {
  const [busy, setBusy] = useState(false);
  return (
    <Button
      onClick={() => {
        setBusy(true);
        onRegenerate()
          .catch((error) => console.warn("could not regenerate", error))
          .finally(() => setBusy(false));
      }}
      disabled={busy}
      aria-label="Regenerate in a new chat"
      title="Regenerate in a new chat"
      className={`${iconButton} size-7`}
    >
      <LuRefreshCw size={13} className={busy ? "animate-spin" : undefined} />
    </Button>
  );
}

function AssistantMessageView({ message, onRespond, onRetry, onRegenerate }: { message: AssistantMessage } & MessageProps) {
  const text = message.state === "running" ? "" : replyText(message);
  const showThinking = message.state === "running" && (message.writing !== undefined || !isBusy(message.parts.at(-1)));

  return (
    <div className="space-y-3">
      {blocksOf(message.parts).map((block, at, blocks) => {
        if (block.kind === "part") return <PartView key={partKey(block.part, block.index)} part={block.part} onRespond={onRespond} onRetry={onRetry} />;
        const steps = block.parts.map((part, offset) => <PartView key={partKey(part, block.first + offset)} part={part} onRespond={onRespond} />);
        // the steps the agent is on right now stay open so they can be followed
        const live = message.state === "running" && at === blocks.length - 1;
        if (live || block.parts.length < 2) return <div key={`steps-${block.first}`} className="space-y-3">{steps}</div>;
        return (
          <StepGroup key={`steps-${block.first}`} parts={block.parts}>
            {steps}
          </StepGroup>
        );
      })}
      {showThinking && <Thinking label={message.writing} />}
      {message.state !== "running" && artifactsOf(message).map((artifact) => <ArtifactCard key={artifact.path} {...artifact} />)}
      <div className="flex items-center gap-1">
        <RunFooter message={message} />
        {text && <CopyButton text={text} label="Copy reply" />}
        {message.state !== "running" && onRegenerate && <RegenerateButton onRegenerate={onRegenerate} />}
      </div>
    </div>
  );
}

function UserMessageView({ message, onEdit }: { message: UserMessage; onEdit?: (content: string) => Promise<void> }) {
  const [editing, setEditing] = useState(false);
  return (
    <div className="group flex flex-col items-end gap-2">
      {message.attachments && message.attachments.length > 0 && <MessageAttachments attachments={message.attachments} />}
      {editing && onEdit ? (
        <MessageEditor initial={message.content} onSave={onEdit} onCancel={() => setEditing(false)} />
      ) : (
        message.content && <p className="max-w-[85%] rounded-2xl bg-subtle px-4 py-2.5 text-sm leading-6 break-words whitespace-pre-wrap">{message.content}</p>
      )}
      {!editing && (message.content || onEdit) && (
        <div className="-mt-1 flex opacity-0 transition-opacity group-focus-within:opacity-100 group-hover:opacity-100 pointer-coarse:opacity-100">
          {onEdit && (
            <Button onClick={() => setEditing(true)} aria-label="Edit message" title="Edit in a new chat" className={`${iconButton} size-7`}>
              <LuPencil size={13} />
            </Button>
          )}
          {message.content && <CopyButton text={message.content} label="Copy message" />}
        </div>
      )}
    </div>
  );
}

export function MessageItem({ message, onRespond, onRetry, onRegenerate, onEdit }: { message: Message } & MessageProps) {
  if (message.role === "user") return <UserMessageView message={message} onEdit={onEdit} />;
  return <AssistantMessageView message={message} onRespond={onRespond} onRetry={onRetry} onRegenerate={onRegenerate} />;
}
