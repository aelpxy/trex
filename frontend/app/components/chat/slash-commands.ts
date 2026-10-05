// a command typed as `/name` at the start of the composer, run instead of sent
export type SlashCommand = {
  name: string;
  description: string;
  // what follows the name, like `<title>`; a command with options offers them as you type
  argument?: string;
  options?: { value: string; label: string }[];
  run: (argument: string) => void;
};

// one entry in the menu: a command, or one of a command's options once its name is typed
export type Suggestion = { id: string; label: string; detail: string; command: SlashCommand; argument?: string };

const parts = (value: string) => {
  const match = /^\/(\S*)(?:\s+(.*))?$/s.exec(value);
  return match ? { name: match[1].toLowerCase(), argument: match[2]?.trim(), spaced: /^\/\S*\s/.test(value) } : null;
};

// what the menu shows for the text so far; nothing once it can't be a command, so it's sent as text
export function suggestionsFor(value: string, commands: SlashCommand[]): Suggestion[] {
  if (value.includes("\n")) return [];
  const typed = parts(value);
  if (!typed) return [];
  if (!typed.spaced) {
    return commands
      .filter((command) => command.name.startsWith(typed.name))
      .map((command) => ({ id: command.name, label: `/${command.name}${command.argument ? ` ${command.argument}` : ""}`, detail: command.description, command }));
  }
  const command = commands.find((candidate) => candidate.name === typed.name);
  if (!command?.options) return [];
  const needle = (typed.argument ?? "").toLowerCase();
  return command.options
    .filter((option) => option.value.toLowerCase().includes(needle) || option.label.toLowerCase().includes(needle))
    .map((option) => ({ id: `${command.name}:${option.value}`, label: option.label, detail: `/${command.name} ${option.value}`, command, argument: option.value }));
}

// the command a finished line names, with what follows it
export function commandFor(value: string, commands: SlashCommand[]): { command: SlashCommand; argument: string } | null {
  const typed = parts(value.trim());
  const command = typed && commands.find((candidate) => candidate.name === typed.name);
  return command ? { command, argument: typed.argument ?? "" } : null;
}
