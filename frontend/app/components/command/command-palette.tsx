import { useEffect, useMemo, useRef, useState, type KeyboardEvent } from "react";
import { useNavigate } from "react-router";
import { Dialog } from "@base-ui/react/dialog";
import type { IconType } from "react-icons";
import { LuCalendarClock, LuFiles, LuFolder, LuMessageSquare, LuMessageSquarePlus, LuSearch } from "react-icons/lu";

import { backdrop, dialogViewport } from "~/components/ui/styles";
import { useWorkspace } from "~/components/workspace/workspace-provider";

import { isMac, OPEN_SEARCH_EVENT } from "./shortcuts";

type Entry = { id: string; group: "Actions" | "Chats"; label: string; detail?: string; icon: IconType; to: string };

const MAX_CHATS = 50;

const ACTIONS: Entry[] = [
  { id: "new-chat", group: "Actions", label: "New chat", icon: LuMessageSquarePlus, to: "/" },
  { id: "library", group: "Actions", label: "Library", icon: LuFiles, to: "/library" },
  { id: "scheduled", group: "Actions", label: "Scheduled", icon: LuCalendarClock, to: "/scheduled" },
];

// earlier matches rank higher, so typing the start of a title finds it first
function rank(text: string, query: string) {
  const at = text.toLowerCase().indexOf(query);
  if (at === -1) return -1;
  return at === 0 ? 0 : /\W/.test(text[at - 1]) ? 1 : 2;
}

export function CommandPalette() {
  const [open, setOpen] = useState(false);
  const [query, setQuery] = useState("");
  const [active, setActive] = useState(0);
  const { projects, recents } = useWorkspace();
  const navigate = useNavigate();
  const list = useRef<HTMLDivElement>(null);

  useEffect(() => {
    const show = () => setOpen(true);
    const onKey = (event: globalThis.KeyboardEvent) => {
      if (event.key.toLowerCase() === "k" && (event.metaKey || event.ctrlKey) && !event.shiftKey && !event.altKey) {
        event.preventDefault();
        setOpen((current) => !current);
      }
    };
    window.addEventListener(OPEN_SEARCH_EVENT, show);
    window.addEventListener("keydown", onKey);
    return () => {
      window.removeEventListener(OPEN_SEARCH_EVENT, show);
      window.removeEventListener("keydown", onKey);
    };
  }, []);

  const chats = useMemo<Entry[]>(
    () => [
      ...recents.map((chat) => ({ id: chat.id, group: "Chats" as const, label: chat.title, icon: LuMessageSquare, to: `/chat/${chat.id}` })),
      ...projects.flatMap((project) =>
        project.chats.map((chat) => ({ id: chat.id, group: "Chats" as const, label: chat.title, detail: project.name, icon: LuFolder, to: `/chat/${chat.id}` })),
      ),
    ],
    [projects, recents],
  );

  const results = useMemo(() => {
    const needle = query.trim().toLowerCase();
    if (!needle) return [...ACTIONS, ...chats.slice(0, MAX_CHATS)];
    const matching = (entries: Entry[]) =>
      entries
        .map((entry) => ({ entry, score: Math.min(...[entry.label, entry.detail ?? ""].map((text) => rank(text, needle)).filter((score) => score >= 0), 9) }))
        .filter(({ score }) => score < 9)
        .sort((a, b) => a.score - b.score)
        .map(({ entry }) => entry);
    return [...matching(ACTIONS), ...matching(chats).slice(0, MAX_CHATS)];
  }, [query, chats]);

  useEffect(() => setActive(0), [query, open]);
  useEffect(() => {
    list.current?.querySelector(`[data-index="${active}"]`)?.scrollIntoView({ block: "nearest" });
  }, [active]);

  function choose(entry: Entry | undefined) {
    if (!entry) return;
    setOpen(false);
    setQuery("");
    navigate(entry.to);
  }

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "ArrowDown") {
      event.preventDefault();
      setActive((index) => Math.min(index + 1, results.length - 1));
    } else if (event.key === "ArrowUp") {
      event.preventDefault();
      setActive((index) => Math.max(index - 1, 0));
    } else if (event.key === "Enter" && !event.nativeEvent.isComposing) {
      event.preventDefault();
      choose(results[active]);
    }
  }

  return (
    <Dialog.Root
      open={open}
      onOpenChange={(next) => {
        setOpen(next);
        if (!next) setQuery("");
      }}
    >
      <Dialog.Portal>
        <Dialog.Backdrop className={backdrop} />
        <Dialog.Viewport className={`${dialogViewport} items-start pt-[14vh]`}>
          <Dialog.Popup className="ui-card glass w-full max-w-xl overflow-hidden shadow-lg outline-none">
            <Dialog.Title className="sr-only">Search</Dialog.Title>
            <div className="flex items-center gap-2.5 border-b border-line px-4">
              <LuSearch size={16} className="shrink-0 text-muted" />
              <input
                value={query}
                onChange={(event) => setQuery(event.target.value)}
                onKeyDown={onKeyDown}
                autoFocus
                role="combobox"
                aria-expanded
                aria-controls="command-results"
                aria-activedescendant={results[active] ? `command-${results[active].group}-${results[active].id}` : undefined}
                placeholder="Search chats or jump to…"
                className="h-12 min-w-0 flex-1 bg-transparent text-sm text-ink outline-none placeholder:text-muted"
              />
            </div>
            <div ref={list} id="command-results" role="listbox" aria-label="Results" className="max-h-[50vh] overflow-y-auto p-1.5">
              {results.length === 0 && <p className="px-3 py-6 text-center text-sm text-muted">No chats match “{query.trim()}”</p>}
              {results.map((entry, index) => {
                const Icon = entry.icon;
                const heading = index === 0 || results[index - 1].group !== entry.group;
                return (
                  <div key={`${entry.group}-${entry.id}`}>
                    {heading && <p className="px-2.5 pt-2 pb-1 text-[11px] font-medium text-muted">{entry.group}</p>}
                    <div
                      id={`command-${entry.group}-${entry.id}`}
                      data-index={index}
                      role="option"
                      aria-selected={index === active}
                      onMouseMove={() => setActive(index)}
                      onClick={() => choose(entry)}
                      className="flex h-9 cursor-pointer items-center gap-2.5 rounded-md px-2.5 text-[13px] text-muted aria-selected:bg-subtle aria-selected:text-ink"
                    >
                      <Icon size={15} className="shrink-0" />
                      <span className="min-w-0 flex-1 truncate">{entry.label}</span>
                      {entry.detail && <span className="max-w-40 shrink-0 truncate text-[11px] text-muted">{entry.detail}</span>}
                    </div>
                  </div>
                );
              })}
            </div>
            <div className="flex items-center gap-3 border-t border-line px-4 py-2 text-[11px] text-muted">
              <span>↑↓ to move</span>
              <span>↵ to open</span>
              <span>esc to close</span>
              <span className="ml-auto">{isMac() ? "⌘⇧O" : "Ctrl+Shift+O"} new chat</span>
            </div>
          </Dialog.Popup>
        </Dialog.Viewport>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
