import { popup } from "~/components/ui/styles";

import type { Suggestion } from "./slash-commands";

export const COMMAND_MENU_ID = "composer-commands";
export const optionId = (suggestion: Suggestion) => `${COMMAND_MENU_ID}-${suggestion.id.replace(/[^a-z0-9-]/gi, "-")}`;

type CommandMenuProps = { suggestions: Suggestion[]; active: number; onPick: (suggestion: Suggestion) => void; onHover: (index: number) => void };

// the commands matching what's typed, above the composer; the textarea keeps focus and moves through
// them with the arrow keys, so this is a listbox it controls
export function CommandMenu({ suggestions, active, onPick, onHover }: CommandMenuProps) {
  return (
    <ul id={COMMAND_MENU_ID} role="listbox" aria-label="Commands" className={`absolute right-0 bottom-full left-0 z-20 mb-2 max-h-72 overflow-y-auto rounded-xl p-1 ${popup}`}>
      {suggestions.map((suggestion, index) => (
        <li
          key={suggestion.id}
          id={optionId(suggestion)}
          role="option"
          aria-selected={index === active}
          // mousedown keeps the textarea focused, so the pick lands back in it
          onMouseDown={(event) => {
            event.preventDefault();
            onPick(suggestion);
          }}
          onMouseMove={() => onHover(index)}
          className="flex cursor-pointer items-baseline gap-3 rounded-lg px-3 py-2 text-[13px] text-muted aria-selected:bg-subtle aria-selected:text-ink"
        >
          <span className="shrink-0 font-mono text-ink">{suggestion.label}</span>
          <span className="min-w-0 truncate text-xs">{suggestion.detail}</span>
        </li>
      ))}
      <li role="presentation" className="px-3 pt-1.5 pb-1 text-[11px] text-muted">
        ↑↓ to choose · Enter or Tab to pick · Esc to close
      </li>
    </ul>
  );
}
