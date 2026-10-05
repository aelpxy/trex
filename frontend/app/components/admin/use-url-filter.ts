import { useSearchParams } from "react-router";

// a text filter kept in the url, so a filtered view can be shared or reloaded
export function useUrlFilter(key = "q") {
  const [params, setParams] = useSearchParams();
  const value = params.get(key) ?? "";
  const setValue = (next: string) =>
    setParams(
      (current) => {
        const updated = new URLSearchParams(current);
        if (next) updated.set(key, next);
        else updated.delete(key);
        return updated;
      },
      { replace: true, preventScrollReset: true },
    );
  return [value, setValue] as const;
}

export const matches = (needle: string, ...fields: (string | null)[]) => {
  const query = needle.trim().toLowerCase();
  return !query || fields.some((field) => field?.toLowerCase().includes(query));
};
