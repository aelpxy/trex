import { Outlet, redirect, useNavigation } from "react-router";
import { Tooltip } from "@base-ui/react/tooltip";
import { MotionConfig } from "motion/react";
import { IconContext } from "react-icons";

import { AppearanceProvider } from "~/components/appearance/appearance-provider";
import { Background } from "~/components/appearance/background";
import { SettingsDialog } from "~/components/appearance/settings-dialog";
import { ChatSkeleton } from "~/components/chat/chat-skeleton";
import { DesktopSidebar } from "~/components/sidebar/desktop-sidebar";
import { MobileHeader } from "~/components/sidebar/mobile-header";
import { setModels } from "~/components/chat/models";
import { WorkspaceProvider } from "~/components/workspace/workspace-provider";
import { getToken, setWorkspaceId } from "~/lib/api";
import { trex } from "~/lib/trex";
import { loadUiState, UiStateProvider } from "~/lib/ui-state";

import type { Route } from "./+types/layout";

const ICON_DEFAULTS = { attr: { "aria-hidden": true } };

// the account loads once; navigating between chats doesn't refetch it
export async function clientLoader() {
  if (!getToken()) throw redirect("/auth");
  const uiState = loadUiState();
  const me = await trex.me();
  const workspace = me.workspaces.find((candidate) => candidate.id === uiState.workspaceId) ?? me.workspaces[0];
  setWorkspaceId(workspace.id);
  const [models, projects, sessions] = await Promise.all([trex.models(), trex.projects(), trex.sessions()]);
  setModels(models);
  return { uiState: { ...uiState, workspaceId: workspace.id }, account: { me, workspaceId: workspace.id, projects, sessions } };
}

export const shouldRevalidate = () => false;

export default function Layout({ loaderData }: Route.ComponentProps) {
  const navigation = useNavigation();
  // a chat just started from the home page already shows its first message, so the skeleton would only flash
  const freshChat = (navigation.location?.state as { fresh?: unknown } | null)?.fresh !== undefined;
  const loadingChat = navigation.state === "loading" && navigation.location.pathname.startsWith("/chat/") && !freshChat;

  return (
    <MotionConfig reducedMotion="user">
      <IconContext.Provider value={ICON_DEFAULTS}>
        <UiStateProvider initial={loaderData.uiState}>
          <AppearanceProvider>
            <WorkspaceProvider account={loaderData.account}>
              <Tooltip.Provider delay={300}>
                <a href="#main" className="sr-only rounded-md bg-surface px-3 py-2 text-sm focus:not-sr-only focus:fixed focus:top-2 focus:left-2 focus:z-50">
                  Skip to content
                </a>
                <Background />
                <div className="flex h-svh overflow-hidden">
                  <DesktopSidebar />
                  <div className="flex min-h-0 min-w-0 flex-1 flex-col">
                    <MobileHeader />
                    <main id="main" tabIndex={-1} className="glass m-2 flex min-h-0 flex-1 flex-col overflow-hidden rounded-xl shadow-sm ring-1 ring-line outline-none">
                      {/* the glass frame never scrolls, otherwise its blur layer would scroll away with the content */}
                      <div className="flex min-h-0 flex-1 flex-col overflow-y-auto">{loadingChat ? <ChatSkeleton /> : <Outlet />}</div>
                    </main>
                  </div>
                </div>
                <SettingsDialog />
              </Tooltip.Provider>
            </WorkspaceProvider>
          </AppearanceProvider>
        </UiStateProvider>
      </IconContext.Provider>
    </MotionConfig>
  );
}
