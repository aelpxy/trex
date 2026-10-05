import type { ReactNode } from "react";
import { Drawer } from "@base-ui/react/drawer";
import { LuX } from "react-icons/lu";

import { Stat } from "./stat";
import { dialogDescription, dialogTitle, iconButton } from "./styles";

type SideDrawerProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  title: ReactNode;
  description?: ReactNode;
  children: ReactNode;
  // pinned below the scrolling body, for the drawer's last-resort action like deleting
  footer?: ReactNode;
};

// a panel that slides in from the right for a record's details and actions
export function SideDrawer({ open, onOpenChange, title, description, children, footer }: SideDrawerProps) {
  return (
    <Drawer.Root open={open} onOpenChange={onOpenChange} swipeDirection="right">
      <Drawer.Portal>
        <Drawer.Backdrop className="fixed inset-0 z-50 bg-black/30 opacity-[calc(1-var(--drawer-swipe-progress))] transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0 data-swiping:duration-0" />
        <Drawer.Viewport className="fixed inset-0 z-50 flex justify-end">
          <Drawer.Popup className="flex h-full w-[28rem] max-w-[calc(100vw-2rem)] translate-x-(--drawer-swipe-movement-x) glass flex-col border-l border-line shadow-lg outline-none transition-transform duration-[220ms] ease-[cubic-bezier(0.32,0.72,0,1)] data-ending-style:translate-x-full data-starting-style:translate-x-full data-swiping:select-none">
            <Drawer.Content className="flex min-h-0 flex-1 flex-col">
              <div className="flex items-start gap-3 border-b border-line px-5 py-4">
                <div className="min-w-0 flex-1">
                  <Drawer.Title className={`${dialogTitle} truncate`}>{title}</Drawer.Title>
                  {description && <Drawer.Description className={`${dialogDescription} truncate`}>{description}</Drawer.Description>}
                </div>
                <Drawer.Close aria-label="Close" className={`${iconButton} -mt-1 -mr-2`}>
                  <LuX size={16} />
                </Drawer.Close>
              </div>
              <div className="min-h-0 flex-1 overflow-y-auto overscroll-contain">{children}</div>
              {footer && <div className="flex shrink-0 items-center justify-end gap-3 border-t border-line px-5 py-3">{footer}</div>}
            </Drawer.Content>
          </Drawer.Popup>
        </Drawer.Viewport>
      </Drawer.Portal>
    </Drawer.Root>
  );
}

type DrawerSectionProps = { title: string; description?: ReactNode; action?: ReactNode; children: ReactNode };

// a titled block inside a drawer, with an optional action beside its title
export function DrawerSection({ title, description, action, children }: DrawerSectionProps) {
  return (
    <section className="border-b border-line px-5 py-4 last:border-0">
      <div className="flex items-start gap-3">
        <div className="min-w-0 flex-1">
          <h3 className="text-sm font-medium">{title}</h3>
          {description && <p className="mt-1 text-xs text-muted">{description}</p>}
        </div>
        {action && <div className="-my-1 shrink-0">{action}</div>}
      </div>
      <div className="mt-3">{children}</div>
    </section>
  );
}

// the key facts about the record, at the top of the drawer
export function DrawerStats({ stats }: { stats: { label: string; value: ReactNode }[] }) {
  return (
    <dl className="grid grid-cols-2 border-b border-line">
      {stats.map((stat, index) => (
        <Stat key={stat.label} label={stat.label} value={stat.value} className={`border-line px-5 py-3 ${index % 2 === 0 ? "border-r" : ""} ${index >= 2 ? "border-t" : ""}`} />
      ))}
    </dl>
  );
}
