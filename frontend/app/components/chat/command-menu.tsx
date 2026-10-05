import { popup } from "~/components/ui/styles";

import type { Suggestion } from "./slash-commands";

export const COMMAND_MENU_ID = "composer-commands";
export const optionId = (suggestion: Suggestion) => `${COMMAND_MENU_ID}-${suggestion.id.replace(/[^a-z0-9-]/gi, "-")}`;

type CommandMenuProps = { suggestions: Suggestion[]; active: number; onPick: (suggestion: Suggestion) => void; onHover: (index: number) => void };

// the commands matching what's typed, above the composer; the textarea keeps focus and moves through
// them with the arrow keys, so this is a listbox it controls
export function CommandMenu({ suggestions, active, onPick, onHover }: CommandMenuProps) {
  return (
    <ul id={COMMAND_MENU_ID} role="listbox" aria-label="Commands" className={`absolute right-0 bottom-full left-0 z-20 mb-2 max-h-72 overflow-y-auto rounded-lg p-1 ${popup}`}>
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
          className="flex h-9 cursor-pointer items-center gap-2.5 rounded-md px-2.5 text-[13px] text-muted aria-selected:bg-subtle aria-selected:text-ink"
        >
          <span className="min-w-0 flex-1 truncate font-mono">{suggestion.label}</span>
          <span className="max-w-64 shrink-0 truncate text-[11px] text-muted">{suggestion.detail}</span>
        </li>
      ))}
      <li role="presentation" className="-mx-1 mt-1 -mb-1 flex items-center gap-3 border-t border-line px-3.5 py-2 text-[11px] text-muted pointer-coarse:hidden">
        <span>↑↓ to move</span>
        <span>↵ or tab to pick</span>
        <span>esc to close</span>
      </li>
    </ul>
  );
}
