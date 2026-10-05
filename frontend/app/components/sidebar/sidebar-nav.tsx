import { NavLink } from "react-router";
import { Tooltip } from "@base-ui/react/tooltip";

import { focusRing, tooltip } from "~/components/ui/styles";

import type { NavItem } from "./config";
import { Fade } from "./fade";
import { railFit } from "./styles";

type SidebarNavProps = {
  items: NavItem[];
  label: string;
  className?: string;
  collapsed?: boolean;
  onNavigate?: () => void;
};

export function SidebarNav({ items, label, className = "", collapsed = false, onNavigate }: SidebarNavProps) {
  return (
    <nav aria-label={label} className={`space-y-0.5 ${className}`}>
      {items.map(({ to, label, icon: Icon }) => (
        <Tooltip.Root key={to} disabled={!collapsed}>
          <Tooltip.Trigger
            render={<NavLink to={to} end onClick={onNavigate} aria-label={collapsed ? label : undefined} />}
            className={({ open }) =>
              `flex h-8 items-center gap-2.5 overflow-hidden whitespace-nowrap rounded-md px-3 text-[13px] hover:bg-subtle hover:text-ink aria-[current=page]:bg-subtle aria-[current=page]:text-ink ${focusRing} ${railFit} ${open ? "bg-subtle text-ink" : "text-muted"}`
            }
          >
            <Icon size={16} className="shrink-0" />
            <Fade show={!collapsed} className="truncate">{label}</Fade>
          </Tooltip.Trigger>
          <Tooltip.Portal>
            <Tooltip.Positioner side="right" sideOffset={8}>
              <Tooltip.Popup className={tooltip}>{label}</Tooltip.Popup>
            </Tooltip.Positioner>
          </Tooltip.Portal>
        </Tooltip.Root>
      ))}
    </nav>
  );
}
