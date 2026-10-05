import { useEffect } from "react";

import { toasts } from "~/lib/toasts";

type DrawerRecord<T> = {
  selected: T | null;
  // the current page's rows, and the record fetched on its own, which outlives a re-sort or page change
  rows: T[];
  fetched: T[] | undefined;
  // the lookup finished with nothing stale in flight
  settled: boolean;
  open: boolean;
  onGone: () => void;
  gone: string;
};

// the freshest copy of the record a drawer shows; a record deleted elsewhere closes the drawer
export function useDrawerRecord<T extends { id: string }>({ selected, rows, fetched, settled, open, onGone, gone }: DrawerRecord<T>): T | null {
  const found = rows.find((row) => row.id === selected?.id) ?? fetched?.find((row) => row.id === selected?.id);
  const missing = open && selected !== null && settled && !found;
  useEffect(() => {
    if (!missing) return;
    onGone();
    toasts.add({ title: gone, type: "error" });
  }, [missing, onGone, gone]);
  // while closing it keeps showing the last copy, so it doesn't empty mid-animation
  return found ?? selected;
}
