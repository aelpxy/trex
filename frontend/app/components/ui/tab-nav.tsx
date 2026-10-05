import { NavLink } from "react-router";

import { focusRing } from "./styles";

export type Tab = { to: string; label: string; end?: boolean };

// a tab-like link; also used for segment filters kept in the url, marked with aria-current="page"
export const tabLink = `h-7 shrink-0 rounded-md px-2.5 text-xs leading-7 font-medium text-muted transition-colors hover:text-ink aria-[current=page]:bg-subtle aria-[current=page]:text-ink ${focusRing}`;

// tabs that are routes, so each one has its own url and loads its own data
export function TabNav({ tabs, label }: { tabs: Tab[]; label: string }) {
  return (
    <nav aria-label={label} className="mt-6 flex gap-1 overflow-x-auto border-b border-line pb-2">
      {tabs.map((tab) => (
        <NavLink
          key={tab.to}
          to={tab.to}
          end={tab.end}
          className={tabLink}
        >
          {tab.label}
        </NavLink>
      ))}
    </nav>
  );
}
