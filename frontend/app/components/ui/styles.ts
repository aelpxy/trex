export const focusRing = "focus-visible:outline-2 focus-visible:-outline-offset-2 focus-visible:outline-accent";
export const focusRingOutset = "focus-visible:outline-2 focus-visible:outline-offset-2 focus-visible:outline-accent";

const badgeBase = "rounded px-1.5 py-0.5 text-[10px] font-medium";
export const badge = `${badgeBase} bg-subtle text-muted`;
export const dangerBadge = `${badgeBase} bg-danger/10 text-danger`;

export const fieldLabel = "mb-1.5 block text-xs font-medium text-muted";

export const iconButton = `inline-flex size-8 shrink-0 cursor-pointer items-center justify-center rounded-md text-muted transition-colors hover:bg-subtle hover:text-ink ${focusRing}`;

const popIn = "origin-(--transform-origin) transition-[transform,opacity] data-ending-style:scale-95 data-ending-style:opacity-0 data-starting-style:scale-95 data-starting-style:opacity-0";

export const popup = `glass border border-line shadow-lg outline-none duration-100 ${popIn}`;
export const tooltip = `glass rounded-md border border-line px-2 py-1 text-xs text-ink shadow-sm duration-100 data-instant:transition-none ${popIn}`;

const menuItemBase = "flex h-8 cursor-pointer items-center gap-2.5 rounded-md px-2.5 text-[13px] text-muted outline-none select-none data-highlighted:bg-subtle";
export const menuItem = `${menuItemBase} data-highlighted:text-ink`;
export const dangerMenuItem = `${menuItemBase} data-highlighted:text-danger`;
export const menuSeparator = "m-1 h-px bg-line";

export const backdrop = "fixed inset-0 z-50 bg-black/30 transition-opacity duration-150 data-ending-style:opacity-0 data-starting-style:opacity-0";
export const dialogViewport = "fixed inset-0 z-50 flex items-center justify-center p-4";
export const dialogPopup = `ui-card glass w-full p-6 shadow-lg outline-none duration-150 ${popIn}`;
export const dialogTitle = "text-base font-medium tracking-tight";
export const dialogDescription = "mt-1 text-sm text-muted";

export const collapsiblePanel = "h-(--collapsible-panel-height) overflow-hidden transition-[height] duration-150 ease-out data-ending-style:h-0 data-starting-style:h-0 [&[hidden]:not([hidden='until-found'])]:hidden";
