import { useRef, useState, type DragEvent } from "react";

// its own type, so only chats dragged from the sidebar are accepted, never links or files
const CHAT_TYPE = "application/x-chat-id";

export function chatDragProps(chatId: string) {
  return {
    draggable: true,
    onDragStart: (event: DragEvent) => {
      event.dataTransfer.setData(CHAT_TYPE, chatId);
      event.dataTransfer.effectAllowed = "move";
    },
  };
}

// a place chats can be dropped on; `over` is true while one hovers it
export function useChatDrop(onDrop: (chatId: string) => void) {
  const [over, setOver] = useState(false);
  // dragenter and dragleave fire for every child crossed, so count them
  const depth = useRef(0);
  const accepts = (event: DragEvent) => event.dataTransfer.types.includes(CHAT_TYPE);

  const props = {
    onDragEnter: (event: DragEvent) => {
      if (!accepts(event)) return;
      event.preventDefault();
      depth.current += 1;
      setOver(true);
    },
    onDragOver: (event: DragEvent) => {
      if (!accepts(event)) return;
      event.preventDefault();
      event.dataTransfer.dropEffect = "move";
    },
    onDragLeave: (event: DragEvent) => {
      if (!accepts(event)) return;
      depth.current = Math.max(0, depth.current - 1);
      if (depth.current === 0) setOver(false);
    },
    onDrop: (event: DragEvent) => {
      if (!accepts(event)) return;
      event.preventDefault();
      depth.current = 0;
      setOver(false);
      const chatId = event.dataTransfer.getData(CHAT_TYPE);
      if (chatId) onDrop(chatId);
    },
  };
  return { over, props };
}
