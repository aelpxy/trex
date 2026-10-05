import { useState } from "react";
import { Drawer } from "@base-ui/react/drawer";
import { Link } from "react-router";
import { LuMenu, LuMessageSquarePlus, LuX } from "react-icons/lu";

import { Brand } from "~/components/ui/brand";
import { iconButton } from "~/components/ui/styles";

import { SidebarContent } from "./sidebar-content";

export function MobileHeader() {
  const [open, setOpen] = useState(false);

  return (
    <header className="relative z-10 mx-2 mt-2 flex h-12 shrink-0 items-center gap-2 rounded-xl border border-line glass px-2 shadow-sm md:hidden">
      <Drawer.Root open={open} onOpenChange={setOpen} swipeDirection="left">
        <Drawer.Trigger aria-label="Open sidebar" className={iconButton}>
          <LuMenu size={16} />
        </Drawer.Trigger>
        <Drawer.Portal>
          <Drawer.Backdrop className="fixed inset-0 bg-black/30 opacity-[calc(1-var(--drawer-swipe-progress))] transition-opacity duration-200 data-ending-style:opacity-0 data-starting-style:opacity-0 data-swiping:duration-0" />
          <Drawer.Viewport className="fixed inset-0 flex">
            <Drawer.Popup className="flex h-full w-60 max-w-[calc(100vw-3rem)] translate-x-(--drawer-swipe-movement-x) flex-col border-r border-line glass outline-none transition-transform duration-[220ms] ease-[cubic-bezier(0.32,0.72,0,1)] data-ending-style:-translate-x-full data-starting-style:-translate-x-full data-swiping:select-none">
              <Drawer.Content className="flex min-h-0 flex-1 flex-col">
                <div className="flex h-14 items-center justify-between px-3">
                  <Drawer.Title render={<span />}><Brand /></Drawer.Title>
                  <Drawer.Close aria-label="Close sidebar" className={iconButton}>
                    <LuX size={16} />
                  </Drawer.Close>
                </div>
                <SidebarContent onNavigate={() => setOpen(false)} />
              </Drawer.Content>
            </Drawer.Popup>
          </Drawer.Viewport>
        </Drawer.Portal>
      </Drawer.Root>
      <Brand />
      <Link to="/" aria-label="New chat" title="New chat" className={`${iconButton} ml-auto`}>
        <LuMessageSquarePlus size={16} />
      </Link>
    </header>
  );
}
