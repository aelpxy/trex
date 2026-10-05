import { Tooltip } from "@base-ui/react/tooltip";
import { LuSearch } from "react-icons/lu";

import { isMac, openSearch } from "~/components/command/shortcuts";
import { focusRing, tooltip } from "~/components/ui/styles";

import { Fade } from "./fade";
import { railFit } from "./styles";

export function SearchButton({ collapsed = false, onNavigate }: { collapsed?: boolean; onNavigate?: () => void }) {
  const shortcut = isMac() ? "⌘K" : "Ctrl K";
  return (
    <Tooltip.Root disabled={!collapsed}>
      <Tooltip.Trigger
        render={<button type="button" />}
        onClick={() => {
          onNavigate?.();
          openSearch();
        }}
        aria-label={collapsed ? "Search" : undefined}
        aria-keyshortcuts={isMac() ? "Meta+K" : "Control+K"}
        className={`flex h-8 w-full cursor-pointer items-center gap-2.5 overflow-hidden whitespace-nowrap rounded-md px-3 text-[13px] text-muted hover:bg-subtle hover:text-ink ${focusRing} ${railFit}`}
      >
        <LuSearch size={16} className="shrink-0" />
        <Fade show={!collapsed} className="flex min-w-0 flex-1 items-center">
          <span className="truncate">Search</span>
          <kbd className="ml-auto font-sans text-[11px] text-muted/80">{shortcut}</kbd>
        </Fade>
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Positioner side="right" sideOffset={8}>
          <Tooltip.Popup className={tooltip}>Search {shortcut}</Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
}
