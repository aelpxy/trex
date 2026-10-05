import type { IconType } from "react-icons";
import { LuCalendarClock, LuFiles, LuMessageSquarePlus } from "react-icons/lu";

export type NavItem = { to: string; label: string; icon: IconType };

export const MAIN_NAV: NavItem[] = [
  { to: "/", label: "New chat", icon: LuMessageSquarePlus },
  { to: "/scheduled", label: "Scheduled", icon: LuCalendarClock },
];
export const LIBRARY_NAV: NavItem[] = [{ to: "/library", label: "Library", icon: LuFiles }];

export const SIDEBAR_ID = "sidebar";
export const DEFAULT_WIDTH = 224;
export const RAIL_WIDTH = 56;
export const MIN_WIDTH = 200;
export const MAX_WIDTH = 360;
export const RESIZE_STEP = 16;

export const SLIDE_TRANSITION = { duration: 0.22, ease: [0.32, 0.72, 0, 1] } as const;
