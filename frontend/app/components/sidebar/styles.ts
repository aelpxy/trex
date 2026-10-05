import { focusRing } from "~/components/ui/styles";

// follows the sidebar's animated width; the variable is unset in the mobile drawer, so the cap falls away there
export const railFit = "w-full max-w-[calc(var(--sidebar-w)-1rem)] transition-colors";

export const sectionItem = `flex h-8 w-full cursor-pointer items-center gap-2.5 rounded-md px-3 text-left text-[13px] text-muted transition-colors hover:bg-subtle hover:text-ink aria-[current=page]:bg-subtle aria-[current=page]:text-ink ${focusRing}`;
