import { useState } from "react";
import { useQuery } from "@tanstack/react-query";
import { Link } from "react-router";
import { Menu } from "@base-ui/react/menu";
import { LuCheck, LuChevronUp, LuCoins, LuLogOut, LuMoon, LuSettings, LuShield, LuUser, LuUserCog } from "react-icons/lu";

import { useTheme } from "~/components/appearance/use-theme";
import { focusRing, menuGroupLabel, menuItem, menuSeparator, menuSwitch, menuSwitchThumb, popup } from "~/components/ui/styles";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { formatUsd } from "~/lib/credits";
import { queries } from "~/lib/queries";

import { Fade } from "./fade";
import { railFit } from "./styles";

function WorkspaceAvatar({ name }: { name: string }) {
  return (
    <span aria-hidden className="flex size-5 shrink-0 items-center justify-center rounded-md bg-subtle text-[10px] font-semibold text-ink ring-1 ring-line">
      {name.charAt(0).toUpperCase()}
    </span>
  );
}

type ProfileMenuProps = { collapsed?: boolean; onNavigate?: () => void };

export function ProfileMenu({ collapsed = false, onNavigate }: ProfileMenuProps) {
  const { theme, setTheme } = useTheme();
  const { workspaces, current, switchWorkspace, profile: PROFILE, logout } = useWorkspace();
  const subtitle = `${current.name} · ${current.plan}`;
  const [open, setOpen] = useState(false);
  // the balance changes with every response, so it's read fresh each time the menu opens
  const credits = useQuery({ ...queries.credits(), enabled: open, staleTime: 0 });
  const balance = credits.data?.balance ?? null;

  return (
    <Menu.Root onOpenChange={setOpen}>
      <Menu.Trigger
        aria-label={collapsed ? `${PROFILE.name}, ${subtitle}` : undefined}
        className={`flex h-11 cursor-pointer items-center gap-2.5 overflow-hidden whitespace-nowrap rounded-md px-1.5 text-left hover:bg-subtle data-popup-open:bg-subtle ${focusRing} ${railFit}`}
      >
        <span className="flex size-7 shrink-0 items-center justify-center rounded-full border border-line bg-subtle text-muted">
          <LuUser size={14} />
        </span>
        <Fade show={!collapsed} className="flex min-w-0 flex-1 items-center gap-2">
          <span className="min-w-0 flex-1 leading-tight">
            <span className="block truncate text-[13px] font-medium">{PROFILE.name}</span>
            <span className="block truncate text-[11px] text-muted">{subtitle}</span>
          </span>
          <LuChevronUp size={14} className="shrink-0 text-muted" />
        </Fade>
      </Menu.Trigger>
      <Menu.Portal>
        <Menu.Positioner side={collapsed ? "right" : "top"} align={collapsed ? "end" : "start"} sideOffset={6} className="z-50">
          <Menu.Popup className={`w-60 min-w-(--anchor-width) rounded-lg p-1 ${popup}`}>
            <div className="px-2.5 py-2">
              <p className="truncate text-[13px] font-medium">{PROFILE.name}</p>
              <p className="truncate text-[11px] text-muted">{subtitle}</p>
              {balance !== null && (
                <p className="mt-1.5 flex items-center gap-1.5 text-[11px] text-muted tabular-nums">
                  <LuCoins size={12} />
                  {formatUsd(balance)} left
                </p>
              )}
            </div>
            <Menu.Separator className={menuSeparator} />
            <Menu.RadioGroup
              value={current.id}
              onValueChange={(id: string) => {
                if (id === current.id) return;
                switchWorkspace(id);
                onNavigate?.();
              }}
            >
              <Menu.GroupLabel className={menuGroupLabel}>Workspaces</Menu.GroupLabel>
              {workspaces.map((workspace) => (
                <Menu.RadioItem key={workspace.id} value={workspace.id} closeOnClick className={menuItem}>
                  <WorkspaceAvatar name={workspace.name} />
                  <span className="min-w-0 flex-1 truncate text-ink">{workspace.name}</span>
                  <span className="text-[11px] text-muted">{workspace.plan}</span>
                  <Menu.RadioItemIndicator keepMounted className="w-3.5 text-ink data-unchecked:invisible">
                    <LuCheck size={14} />
                  </Menu.RadioItemIndicator>
                </Menu.RadioItem>
              ))}
            </Menu.RadioGroup>
            <Menu.Separator className={menuSeparator} />
            <Menu.CheckboxItem checked={theme === "dark"} onCheckedChange={(dark) => setTheme(dark ? "dark" : "light")} className={`group ${menuItem}`}>
              <LuMoon size={14} />
              <span className="flex-1">Dark mode</span>
              <span aria-hidden className={menuSwitch}>
                <span className={menuSwitchThumb} />
              </span>
            </Menu.CheckboxItem>
            <Menu.LinkItem render={<Link to="/account" onClick={onNavigate} />} closeOnClick className={menuItem}><LuUserCog size={14} />Account</Menu.LinkItem>
            <Menu.LinkItem render={<Link to="/account/appearance" onClick={onNavigate} />} closeOnClick className={menuItem}><LuSettings size={14} />Appearance</Menu.LinkItem>
            {PROFILE.role === "admin" && (
              <Menu.LinkItem render={<Link to="/admin" onClick={onNavigate} />} closeOnClick className={menuItem}><LuShield size={14} />Admin</Menu.LinkItem>
            )}
            <Menu.Separator className={menuSeparator} />
            <Menu.Item onClick={logout} className={menuItem}><LuLogOut size={14} />Log out</Menu.Item>
          </Menu.Popup>
        </Menu.Positioner>
      </Menu.Portal>
    </Menu.Root>
  );
}
