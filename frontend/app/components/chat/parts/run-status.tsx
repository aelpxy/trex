import { LuBox, LuLoaderCircle } from "react-icons/lu";

import type { StatusPart } from "../types";

export function RunStatus({ part }: { part: StatusPart }) {
  return (
    <p role="status" className="flex items-center gap-2 text-xs text-muted">
      {part.done ? <LuBox size={13} /> : <LuLoaderCircle size={13} className="animate-spin" />}
      {part.label}
      {!part.done && "…"}
    </p>
  );
}
