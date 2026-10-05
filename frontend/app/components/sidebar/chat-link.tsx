import { NavLink } from "react-router";

import type { Chat } from "~/lib/workspace";

import { ItemContextMenu } from "./item-context-menu";
import { sectionItem } from "./styles";

type ChatLinkProps = { chat: Chat; onNavigate?: () => void };

export function ChatLink({ chat, onNavigate }: ChatLinkProps) {
  return (
    <ItemContextMenu target={{ kind: "chat", id: chat.id, name: chat.title }}>
      <NavLink to={`/chat/${chat.id}`} onClick={onNavigate} className={sectionItem}>
        <span className="truncate">{chat.title}</span>
      </NavLink>
    </ItemContextMenu>
  );
}
