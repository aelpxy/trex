import { Skeleton } from "~/components/ui/skeleton";

export function ChatSkeleton() {
  return (
    <div role="status" aria-label="Loading chat" className="flex flex-1 flex-col">
      <div className="flex h-12 items-center px-4">
        <Skeleton className="h-4 w-40" />
      </div>
      <div className="mx-auto w-full max-w-2xl space-y-8 px-4 pt-8">
        <div className="flex justify-end">
          <Skeleton className="h-10 w-2/5 rounded-2xl" />
        </div>
        <div className="space-y-2.5">
          <Skeleton className="h-3.5 w-full" />
          <Skeleton className="h-3.5 w-11/12" />
          <Skeleton className="h-3.5 w-3/5" />
        </div>
        <div className="flex justify-end">
          <Skeleton className="h-10 w-1/3 rounded-2xl" />
        </div>
        <div className="space-y-2.5">
          <Skeleton className="h-3.5 w-full" />
          <Skeleton className="h-28 w-full rounded-xl" />
          <Skeleton className="h-3.5 w-2/3" />
        </div>
      </div>
    </div>
  );
}
