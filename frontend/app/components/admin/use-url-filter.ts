import { useCallback } from "react";
import { useSearchParams } from "react-router";

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

// the page and search a paged admin list's loader fetches
export function listParams(request: Request) {
  const params = new URL(request.url).searchParams;
  return { page: Math.max(1, Number(params.get("page")) || 1), search: params.get("q")?.trim() ?? "" };
}

export const matches = (needle: string, ...fields: (string | null)[]) => {
  const query = needle.trim().toLowerCase();
  return !query || fields.some((field) => field?.toLowerCase().includes(query));
};
