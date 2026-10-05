import { LuCircleAlert, LuRotateCcw } from "react-icons/lu";

import { Button } from "~/components/ui/button";

import type { ErrorPart } from "../types";

export function ErrorCard({ part, onRetry }: { part: ErrorPart; onRetry?: () => void }) {
  return (
    <div role="alert" className="flex items-start gap-2.5 rounded-xl border border-danger/30 bg-danger/5 px-3 py-2.5">
      <LuCircleAlert size={15} className="mt-0.5 shrink-0 text-danger" />
      <div className="min-w-0 flex-1 text-[13px] leading-5">
        <p className="font-medium">{part.title}</p>
        {part.detail && <p className="mt-0.5 break-words text-muted">{part.detail}</p>}
      </div>
      {part.retry && onRetry && (
        <Button variant="quiet" onClick={onRetry} className="h-7 shrink-0 px-2.5 text-xs">
          <LuRotateCcw size={12} />
          Retry
        </Button>
      )}
    </div>
  );
}
