import { useEffect } from "react";
import { redirect, useLocation } from "react-router";

import { ChatView } from "~/components/chat/chat-view";
import type { FreshChat } from "~/components/chat/use-chat";
import { ApiError } from "~/lib/api";
import { pageTitle } from "~/lib/meta";
import { trex } from "~/lib/trex";
import { UNTITLED } from "~/lib/workspace";

import type { Route } from "./+types/chat";

export async function clientLoader({ params }: Route.ClientLoaderArgs) {
  try {
    // usage only fills in the message footers, so a chat still opens without it
    const usage = trex.usage(params.chatId).catch((error) => {
      console.warn("could not load usage", error);
      return [];
    });
    const [session, items, access] = await Promise.all([trex.session(params.chatId), trex.items(params.chatId), trex.accessRequests(params.chatId)]);
    return { chat: { session, items, access, usage: await usage } };
  } catch (error) {
    if (error instanceof ApiError && error.status === 404) throw redirect("/");
    throw error;
  }
}

export const meta = ({ loaderData }: Route.MetaArgs) => pageTitle(loaderData?.chat.session.title ?? UNTITLED);

export default function Chat({ params, loaderData }: Route.ComponentProps) {
  const location = useLocation();
  const fresh = (location.state as { fresh?: FreshChat } | null)?.fresh;

  // browsers keep navigation state across reloads, which would replay the first run again
  useEffect(() => {
    if (fresh) window.history.replaceState({ ...window.history.state, usr: null }, "");
  }, [fresh]);
  return <ChatView key={params.chatId} chatId={params.chatId} data={loaderData.chat} fresh={fresh} />;
}
