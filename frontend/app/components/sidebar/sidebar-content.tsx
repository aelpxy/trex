import { ScrollArea } from "@base-ui/react/scroll-area";
import { Separator } from "@base-ui/react/separator";

import { useWorkspace } from "~/components/workspace/workspace-provider";

import { ChatLink } from "./chat-link";
import { LIBRARY_NAV, MAIN_NAV } from "./config";
import { Fade } from "./fade";
import { NewProjectDialog } from "./new-project-dialog";
import { ProfileMenu } from "./profile-menu";
import { ProjectFolder } from "./project-folder";
import { SearchButton } from "./search-button";
import { SidebarNav } from "./sidebar-nav";
import { SidebarSection } from "./sidebar-section";

type SidebarContentProps = { collapsed?: boolean; onNavigate?: () => void };

export function SidebarContent({ collapsed = false, onNavigate }: SidebarContentProps) {
  const { projects, recents } = useWorkspace();
  return (
    <>
      <ScrollArea.Root className="relative min-h-0 flex-1">
        <ScrollArea.Viewport className="h-full overscroll-contain py-1">
          <SidebarNav items={MAIN_NAV} label="Main" className="px-2" collapsed={collapsed} onNavigate={onNavigate} />
          <div className="mt-0.5 px-2">
            <SearchButton collapsed={collapsed} onNavigate={onNavigate} />
          </div>
          <Fade show={!collapsed} as="div" className="mt-4 block space-y-3 px-2">
            <SidebarSection
              id="projects"
              title="Projects"
              action={<NewProjectDialog />}
            >
              {projects.map((project) => (
                <li key={project.id}>
                  <ProjectFolder project={project} onNavigate={onNavigate} />
                </li>
              ))}
            </SidebarSection>
            <SidebarSection id="recents" title="Recents">
              {recents.map((chat) => (
                <li key={chat.id}>
                  <ChatLink chat={chat} onNavigate={onNavigate} />
                </li>
              ))}
              {recents.length === 0 && <li className="px-3 py-1.5 text-xs text-muted">No chats yet</li>}
            </SidebarSection>
          </Fade>
        </ScrollArea.Viewport>
        <ScrollArea.Scrollbar className="m-1 flex w-1 justify-center rounded-full opacity-0 transition-opacity data-hovering:opacity-100 data-scrolling:opacity-100">
          <ScrollArea.Thumb className="w-full rounded-full bg-line" />
        </ScrollArea.Scrollbar>
      </ScrollArea.Root>
      <SidebarNav items={LIBRARY_NAV} label="Library" className="p-2" collapsed={collapsed} onNavigate={onNavigate} />
      <Separator className="mx-2 h-px bg-line" />
      <div className="p-2">
        <ProfileMenu collapsed={collapsed} onNavigate={onNavigate} />
      </div>
    </>
  );
}
