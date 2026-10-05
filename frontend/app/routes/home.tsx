import { ChatView } from "~/components/chat/chat-view";
import { pageTitle } from "~/lib/meta";

export const meta = () => pageTitle("New chat");

export default function Home() {
  return <ChatView />;
}
