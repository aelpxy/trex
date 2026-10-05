import { useCallback } from "react";
import { useSearchParams } from "react-router";

import type { SortingState } from "~/components/ui/data-table";

// a text filter kept in the url, so a filtered view can be shared or reloaded; a new filter starts
// again from the first page
export function useUrlFilter(key = "q") {
  const [params, setParams] = useSearchParams();
  const value = params.get(key) ?? "";
  const setValue = useCallback(
    (next: string) =>
      setParams(
        (current) => {
          const updated = new URLSearchParams(current);
          if (next) updated.set(key, next);
          else updated.delete(key);
          updated.delete("page");
          return updated;
        },
        { replace: true, preventScrollReset: true },
      ),
    [key, setParams],
  );
  return [value, setValue] as const;
}

// a server-sorted table's sort in the url as `name` or `-name`; a new sort starts again from the
// first page
export function useUrlSort() {
  const [params, setParams] = useSearchParams();
  const raw = params.get("sort") ?? "";
  const sorting: SortingState = raw ? [{ id: raw.replace(/^-/, ""), desc: raw.startsWith("-") }] : [];
  const setSorting = useCallback(
    (next: SortingState) =>
      setParams(
        (current) => {
          const updated = new URLSearchParams(current);
          const [first] = next;
          if (first) updated.set("sort", `${first.desc ? "-" : ""}${first.id}`);
          else updated.delete("sort");
          updated.delete("page");
          return updated;
        },
        { replace: true, preventScrollReset: true },
      ),
    [setParams],
  );
  return { sort: raw, sorting, setSorting };
}

// the page, search and sort a paged admin list's loader fetches
export function listParams(request: Request) {
  const params = new URL(request.url).searchParams;
  return { page: Math.max(1, Number(params.get("page")) || 1), search: params.get("q")?.trim() ?? "", sort: params.get("sort") ?? "" };
}

export const matches = (needle: string, ...fields: (string | null)[]) => {
  const query = needle.trim().toLowerCase();
  return !query || fields.some((field) => field?.toLowerCase().includes(query));
};
