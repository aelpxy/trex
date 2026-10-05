import { Navigate } from "react-router";

import { ChatView } from "~/components/chat/chat-view";
import { ProjectDetails } from "~/components/projects/project-details";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { pageTitle } from "~/lib/meta";

import type { Route } from "./+types/project";

export const meta = () => pageTitle("Project");

export default function ProjectPage({ params }: Route.ComponentProps) {
  const { projects } = useWorkspace();
  const project = projects.find((candidate) => candidate.id === params.projectId);
  // deleted, or from another workspace
  if (!project) return <Navigate to="/" replace />;

  return (
    <ChatView key={project.id} project={project}>
      <ProjectDetails project={project} />
    </ChatView>
  );
}
