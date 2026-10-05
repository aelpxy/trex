import { useEffect, useMemo, useRef } from "react";

import { FilePanel } from "~/components/files/file-panel";
import { FilesButton } from "~/components/files/files-button";
import { FilesProvider, useFiles } from "~/components/files/files-provider";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { previewOf } from "~/lib/preview";

import { ChatHeader } from "./chat-header";
import { Composer } from "./composer";
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
  return last.parts.flatMap((part) => (part.type === "tool" && part.state === "running" && part.name !== "shell" && part.input.path ? [part.input.path] : []));
}

// like an artifact, a page or document the agent starts writing opens in the panel, once per file
function OpenWhileWriting({ writing }: { writing: string[] }) {
  const { open } = useFiles();
  const opened = useRef(new Set<string>());
  useEffect(() => {
    const path = writing.find((candidate) => previewOf(candidate) && !opened.current.has(candidate));
    if (!path) return;
    opened.current.add(path);
    open(path);
  }, [writing, open]);
  return null;
}

type ChatViewProps = { chatId?: string; data?: ChatData; fresh?: FreshChat };

export function ChatView({ chatId, data, fresh }: ChatViewProps) {
  const { requestDelete } = useWorkspace();
  const { settings, update } = useChatSettings(data?.session);
  const { messages, running, queued, send, stop, respond, title, rename: setTitle } = useChat({ chatId, data, fresh, settings });
  const { scroller, atBottom, follow, scrollToBottom } = useFollowScroll(messages);
  const files = useMemo(() => filesFrom(messages), [messages]);
  const writingNow = writingFrom(messages);
  const writingKey = writingNow.join("\n");
  const writing = useMemo(() => (writingKey ? writingKey.split("\n") : []), [writingKey]);

  function sendAndFollow(content: string, attachments: OutgoingAttachment[] = [], interrupt = false) {
    follow();
    send(content, attachments, interrupt);
  }

  const composer = <Composer streaming={running} settings={settings} queued={queued} onSettingsChange={update} onSend={sendAndFollow} onStop={stop} />;

  return (
    <FilesProvider sessionId={chatId} generated={files} writing={writing} running={running}>
      <OpenWhileWriting writing={writing} />
      <div className="flex min-h-0 flex-1">
        <div ref={scroller} className="flex min-w-0 flex-1 flex-col overflow-y-auto overscroll-contain">
          {messages.length > 0 && <h1 className="sr-only">{title}</h1>}
          {title && <ChatHeader title={title} onRename={setTitle} onDelete={chatId ? () => requestDelete({ kind: "chat", id: chatId, name: title }) : undefined} actions={<FilesButton />} />}
          {messages.length === 0 ? (
            <div className="flex flex-1 flex-col items-center justify-center px-4 pb-[12vh]">
              <div className="w-full max-w-2xl">
                <h1 className="mb-6 text-center text-2xl font-medium tracking-tight">{title ? "Continue the conversation" : "What are we building?"}</h1>
                {composer}
                {!title && <Suggestions onPick={sendAndFollow} />}
              </div>
            </div>
          ) : (
            <>
              <div role="log" aria-label="Conversation" aria-busy={running} className="mx-auto w-full max-w-2xl flex-1 space-y-6 px-4 pt-6 pb-4">
                {messages.map((message) => (
                  <MessageItem key={message.id} message={message} onRespond={respond} />
                ))}
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
        <FilePanel />
      </div>
    </FilesProvider>
  );
}
