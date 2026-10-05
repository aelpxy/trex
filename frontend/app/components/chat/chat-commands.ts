import { useNavigate } from "react-router";

import { toasts } from "~/lib/toasts";
import type { Project } from "~/lib/workspace";

import { effortsFor, MODELS, supportsFast, type ChatSettings } from "./models";
import type { SlashCommand } from "./slash-commands";
import type { Message } from "./types";

type ChatCommandOptions = {
  chatId?: string;
  project?: Project;
  messages: Message[];
  running: boolean;
  settings: ChatSettings;
  update: (patch: Partial<ChatSettings>) => void;
  compact: () => void;
  rename: (title: string) => void;
  retry: () => void;
  stop: () => void;
};

// matches what the user typed against an option's value or label, ignoring case
const findOption = (options: { value: string; label: string }[], typed: string) => {
  const needle = typed.trim().toLowerCase();
  return options.find((option) => option.value.toLowerCase() === needle || option.label.toLowerCase() === needle);
};

// the slash commands that make sense in this chat right now; ones that can't run aren't offered
export function useChatCommands({ chatId, project, messages, running, settings, update, compact, rename, retry, stop }: ChatCommandOptions): SlashCommand[] {
  const navigate = useNavigate();
  const last = messages.at(-1);
  const canRetry = !running && last?.role === "assistant" && last.parts.some((part) => part.type === "error" && part.retry);
  const efforts = effortsFor(settings.model);

  const commands: (SlashCommand | false)[] = [
    Boolean(chatId) &&
      !running && {
        name: "compact",
        description: "Summarize the conversation to free up context",
        run: compact,
      },
    {
      name: "new",
      description: project ? `Start a new chat in ${project.name}` : "Start a new chat",
      run: () => navigate(project ? `/projects/${project.id}` : "/"),
    },
    {
      name: "model",
      description: "Switch the model for the next message",
      argument: "<name>",
      options: MODELS.map((model) => ({ value: model.value, label: model.label })),
      run: (typed) => {
        const model = findOption(MODELS, typed);
        if (!model) return toasts.add({ title: `No model called “${typed}”`, type: "error" });
        update({ model: model.value });
        toasts.add({ title: `Using ${model.label}`, type: "success" });
      },
    },
    efforts.length > 1 && {
      name: "effort",
      description: "Set how long the model thinks",
      argument: "<level>",
      options: efforts.map((effort) => ({ value: effort.value, label: effort.label })),
      run: (typed) => {
        const effort = findOption(efforts, typed);
        if (!effort) return toasts.add({ title: `No effort level called “${typed}”`, type: "error" });
        update({ effort: effort.value });
        toasts.add({ title: `Thinking effort: ${effort.label}`, type: "success" });
      },
    },
    supportsFast(settings.model) && {
      name: "fast",
      description: settings.fast ? "Turn fast mode off" : "Turn fast mode on, at a higher price",
      run: () => {
        update({ fast: !settings.fast });
        toasts.add({ title: settings.fast ? "Fast mode off" : "Fast mode on", type: "success" });
      },
    },
    {
      name: "autoapprove",
      description: settings.autoApprove ? "Ask before allowing network access again" : "Approve network access without asking",
      run: () => {
        update({ autoApprove: !settings.autoApprove });
        toasts.add({ title: settings.autoApprove ? "Auto-approve off" : "Auto-approve on", type: "success" });
      },
    },
    Boolean(chatId) && {
      name: "rename",
      description: "Rename this chat",
      argument: "<title>",
      run: (title) => rename(title.trim()),
    },
    canRetry && {
      name: "retry",
      description: "Continue the run that failed",
      run: retry,
    },
    running && {
      name: "stop",
      description: "Stop the agent",
      run: stop,
    },
  ];
  return commands.filter((command): command is SlashCommand => command !== false);
}
