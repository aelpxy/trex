import { createContext, use, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

export type UiState = {
  sidebarCollapsed: boolean;
  sidebarWidth: number | null;
  closedSections: string[];
  openProjects: string[];
  workspaceId: string | null;
};

export const DEFAULT_UI_STATE: UiState = { sidebarCollapsed: false, sidebarWidth: null, closedSections: [], openProjects: [], workspaceId: null };

const STORAGE_KEY = "trex-ui";

const isStringList = (value: unknown): value is string[] => Array.isArray(value) && value.every((item) => typeof item === "string");

// read before first render by the layout's clientLoader so the saved layout shows without a jump
export function loadUiState(): UiState {
  try {
    const parsed = JSON.parse(localStorage.getItem(STORAGE_KEY) ?? "null");
    if (!parsed) return DEFAULT_UI_STATE;
    return {
      sidebarCollapsed: parsed.sidebarCollapsed === true,
      sidebarWidth: Number.isFinite(parsed.sidebarWidth) ? parsed.sidebarWidth : null,
      closedSections: isStringList(parsed.closedSections) ? parsed.closedSections : [],
      openProjects: isStringList(parsed.openProjects) ? parsed.openProjects : [],
      workspaceId: typeof parsed.workspaceId === "string" ? parsed.workspaceId : null,
    };
  } catch (error) {
    console.warn("could not read ui state", error);
    return DEFAULT_UI_STATE;
  }
}

function saveUiState(state: UiState) {
  try {
    localStorage.setItem(STORAGE_KEY, JSON.stringify(state));
  } catch (error) {
    console.warn("could not save ui state", error);
  }
}

export const toggleItem = (list: string[], item: string, include: boolean) => (include ? [...new Set([...list, item])] : list.filter((entry) => entry !== item));

type UiStateContextValue = {
  state: UiState;
  update: (patch: Partial<UiState> | ((current: UiState) => Partial<UiState>)) => void;
};

const UiStateContext = createContext<UiStateContextValue | null>(null);

export function useUiState() {
  const value = use(UiStateContext);
  if (!value) throw new Error("useUiState must be used inside UiStateProvider");
  return value;
}

export function UiStateProvider({ initial, children }: { initial: UiState; children: ReactNode }) {
  const [state, setState] = useState(initial);
  const changed = useRef(false);

  useEffect(() => {
    if (changed.current) saveUiState(state);
  }, [state]);

  const update = useCallback<UiStateContextValue["update"]>((patch) => {
    changed.current = true;
    setState((current) => ({ ...current, ...(typeof patch === "function" ? patch(current) : patch) }));
  }, []);

  const value = useMemo(() => ({ state, update }), [state, update]);
  return <UiStateContext value={value}>{children}</UiStateContext>;
}
