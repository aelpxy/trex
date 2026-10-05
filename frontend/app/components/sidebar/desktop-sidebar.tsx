import { Button } from "@base-ui/react/button";
import { motion, useReducedMotion } from "motion/react";
import { LuPanelLeft } from "react-icons/lu";

import { Brand } from "~/components/ui/brand";
import { iconButton } from "~/components/ui/styles";
import { useUiState } from "~/lib/ui-state";

import { DEFAULT_WIDTH, MAX_WIDTH, MIN_WIDTH, RAIL_WIDTH, RESIZE_STEP, SIDEBAR_ID, SLIDE_TRANSITION } from "./config";
import { Fade } from "./fade";
import { SidebarContent } from "./sidebar-content";
import { useResizableWidth } from "./use-resizable-width";

export function DesktopSidebar() {
  const { state, update } = useUiState();
  const collapsed = state.sidebarCollapsed;
  const { width, resizing, reset, handlers } = useResizableWidth({
    initial: state.sidebarWidth ?? DEFAULT_WIDTH,
    defaultValue: DEFAULT_WIDTH,
    min: MIN_WIDTH,
    max: MAX_WIDTH,
    step: RESIZE_STEP,
    onCommit: (sidebarWidth) => update({ sidebarWidth }),
  });
  const reduceMotion = useReducedMotion();
  const instant = resizing || reduceMotion;

  return (
    <motion.aside
      id={SIDEBAR_ID}
      aria-label="Sidebar"
      initial={false}
      animate={{ "--sidebar-w": `${collapsed ? RAIL_WIDTH : width}px` }}
      transition={instant ? { duration: 0 } : SLIDE_TRANSITION}
      className="sticky top-0 hidden h-svh w-[calc(var(--sidebar-w)+0.5rem)] shrink-0 py-2 pl-2 md:block"
    >
      <div className="relative h-full w-(--sidebar-w) overflow-hidden rounded-xl glass shadow-sm ring-1 ring-line">
        {/* fixed width so the content is clipped instead of reflowing while the sidebar animates */}
        <div className="flex h-full flex-col" style={{ width }}>
          <div className="flex h-14 items-center gap-2 px-3">
            <Fade show={!collapsed} className="flex"><Brand /></Fade>
            <div className="ml-auto" style={{ transform: `translateX(calc(var(--sidebar-w) - ${width}px))` }}>
              <Button
                onClick={() => update((current) => ({ sidebarCollapsed: !current.sidebarCollapsed }))}
                aria-label={collapsed ? "Expand sidebar" : "Collapse sidebar"}
                aria-expanded={!collapsed}
                aria-controls={SIDEBAR_ID}
                className={iconButton}
              >
                <LuPanelLeft size={16} />
              </Button>
            </div>
          </div>
          <SidebarContent collapsed={collapsed} />
        </div>
        {!collapsed && (
          <div
            role="separator"
            aria-orientation="vertical"
            aria-label="Resize sidebar"
            aria-controls={SIDEBAR_ID}
            aria-valuenow={width}
            aria-valuemin={MIN_WIDTH}
            aria-valuemax={MAX_WIDTH}
            tabIndex={0}
            title="Drag to resize, double-click to reset"
            onDoubleClick={reset}
            {...handlers}
            className={`absolute inset-y-3 right-0 w-1 cursor-col-resize rounded-full touch-none transition-colors hover:bg-line focus-visible:bg-accent focus-visible:outline-none ${resizing ? "bg-line" : ""}`}
          />
        )}
      </div>
    </motion.aside>
  );
}
