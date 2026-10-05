import { useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { FilePanel } from "~/components/files/file-panel";
import { PreviewPanel } from "~/components/files/preview-panel";
import { FilesButton } from "~/components/files/files-button";
import { FilesProvider, useFiles } from "~/components/files/files-provider";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { previewOf } from "~/lib/preview";
import type { Project } from "~/lib/workspace";

import { ChatHeader } from "./chat-header";
import { Composer } from "./composer";
import { DropZone } from "./drop-zone";
import { MessageItem } from "./message-item";
import { ScrollToBottom } from "./scroll-to-bottom";
import { Suggestions } from "./suggestions";
import { useChat, type ChatData, type FreshChat, type OutgoingAttachment } from "./use-chat";
import { useChatSettings } from "./use-chat-settings";
import type { Message } from "./types";
import { useFollowScroll } from "./use-follow-scroll";

function filesFrom(messages: Message[]) {
  const files: Record<string, string> = {};
  for (const message of messages) {
    if (message.role !== "assistant") continue;
    for (const part of message.parts) {
      if (part.type !== "tool" || !part.input.path) continue;
      if (part.name === "write_file") files[part.input.path] = part.output.replace(/\n$/, "");
      if (part.name === "edit_file" && part.input.after !== undefined) files[part.input.path] = part.input.after;
    }
  }
  return files;
}

// files tool calls are writing right now, which the panel shows as code until they're done
function writingFrom(messages: Message[]) {
  const last = messages.at(-1);
  if (last?.role !== "assistant" || last.state !== "running") return [];
  return last.parts.flatMap((part) => (part.type === "tool" && part.state === "running" && (part.name === "write_file" || part.name === "edit_file") && part.input.path ? [part.input.path] : []));
}

// the panel covers the whole chat on narrow screens, so it only opens itself when there's room
const roomForPanel = () => window.matchMedia("(min-width: 768px)").matches;

// like an artifact, a page or document the agent starts writing opens in the panel, once per file
function OpenWhileWriting({ writing }: { writing: string[] }) {
  const { open } = useFiles();
  const opened = useRef(new Set<string>());
  useEffect(() => {
    if (!roomForPanel()) return;
    const path = writing.find((candidate) => previewOf(candidate) && !opened.current.has(candidate));
    if (!path) return;
    opened.current.add(path);
    open(path);
  }, [writing, open]);
  return null;
}

// a preview the agent opens during a run shows right away, like a written artifact
function OpenLivePreviews({ messages }: { messages: Message[] }) {
  const { openPreview } = useFiles();
  const last = messages.at(-1);
  const live = last?.role === "assistant" && last.state === "running" ? last.parts.flatMap((part) => (part.type === "preview" ? [`${part.port}${part.path}`] : [])) : [];
  const seen = useRef<Set<string> | null>(null);
  const key = live.join("\n");
  useEffect(() => {
    const opened = key ? key.split("\n") : [];
    // previews from before the page loaded aren't reopened
    if (seen.current === null) {
      seen.current = new Set(opened);
      return;
    }
    const next = opened.find((preview) => !seen.current!.has(preview));
    if (!next || !roomForPanel()) return;
    seen.current.add(next);
    const [, port, path] = next.match(/^(\d+)(.*)$/) ?? [];
    void openPreview(Number(port), path || "/");
  }, [key, openPreview]);
  return null;
}

// a new chat started from a project page belongs to the project, which fills the empty state
type ChatViewProps = { chatId?: string; data?: ChatData; fresh?: FreshChat; project?: Project; children?: ReactNode };

export function ChatView({ chatId, data, fresh, project: newIn, children }: ChatViewProps) {
  const { requestDelete, projects } = useWorkspace();
  const project = newIn ?? projects.find((candidate) => candidate.chats.some((chat) => chat.id === chatId));
  const { settings, update } = useChatSettings(data?.session);
  const { messages, running, queued, send, stop, retry, branch, respond, title, rename: setTitle } = useChat({ chatId, data, fresh, settings, projectId: newIn?.id });
  const { scroller, atBottom, follow, scrollToBottom } = useFollowScroll(messages);
  const files = useMemo(() => filesFrom(messages), [messages]);
  const [dropped, setDropped] = useState<{ files: File[]; id: number }>();
  const writingNow = writingFrom(messages);
  const writingKey = writingNow.join("\n");
  const writing = useMemo(() => (writingKey ? writingKey.split("\n") : []), [writingKey]);

  function sendAndFollow(content: string, attachments: OutgoingAttachment[] = [], interrupt = false) {
    follow();
    send(content, attachments, interrupt);
  }

  const composer = <Composer dropped={dropped} streaming={running} settings={settings} queued={queued} onSettingsChange={update} onSend={sendAndFollow} onStop={stop} />;

  return (
    <FilesProvider sessionId={chatId} generated={files} writing={writing} running={running}>
      <OpenWhileWriting writing={writing} />
      <OpenLivePreviews messages={messages} />
      <div className="flex min-h-0 flex-1 overflow-hidden">
        <DropZone onFiles={(files) => setDropped((current) => ({ files, id: (current?.id ?? 0) + 1 }))}>
          <div ref={scroller} className="flex min-w-0 flex-1 flex-col overflow-y-auto overscroll-contain">
            {messages.length > 0 && <h1 className="sr-only">{title}</h1>}
            {title && <ChatHeader title={title} chatId={chatId} project={project} onRename={setTitle} onDelete={chatId ? () => requestDelete({ kind: "chat", id: chatId, name: title }) : undefined} actions={<FilesButton />} />}
            {messages.length === 0 ? (
              <div className="flex flex-1 flex-col items-center justify-center px-4 pb-[12vh]">
                <div className="w-full max-w-2xl">
                  <h1 className="mb-6 text-center text-2xl font-medium tracking-tight">{title ? "Continue the conversation" : (newIn?.name ?? "What are we building?")}</h1>
                  {composer}
                  {children ?? (!title && <Suggestions onPick={sendAndFollow} />)}
                </div>
              </div>
            ) : (
              <>
                <div role="log" aria-label="Conversation" aria-busy={running} className="mx-auto w-full max-w-2xl flex-1 space-y-6 px-4 pt-6 pb-4">
                  {messages.map((message, index) => {
                    // branches name a message by its place among the user's messages, as the server counts them
                    const userIndex = messages.slice(0, index + 1).filter((earlier) => earlier.role === "user").length - 1;
                    const canBranch = Boolean(chatId) && userIndex >= 0;
                    return (
                      <MessageItem
                        key={message.id}
                        message={message}
                        onRespond={respond}
                        onRetry={index === messages.length - 1 ? retry : undefined}
                        onEdit={canBranch && message.role === "user" ? (content) => branch(userIndex, content) : undefined}
                        onRegenerate={canBranch && message.role === "assistant" ? () => branch(userIndex) : undefined}
                      />
                    );
                  })}
                    </div>
                <div className="sticky bottom-0 px-4 pt-2 pb-4">
                  <div className="relative mx-auto w-full max-w-2xl">
                    <ScrollToBottom visible={!atBottom} onClick={scrollToBottom} />
                    {composer}
                  </div>
                </div>
              </>
            )}
          </div>
        </DropZone>
        <FilePanel />
        <PreviewPanel />
      </div>
    </FilesProvider>
  );
}
