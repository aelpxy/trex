import type { ReactNode } from "react";
import type { IconType } from "react-icons";

type EmptyStateProps = { icon: IconType; title: string; description: string; action?: ReactNode };

export function EmptyState({ icon: Icon, title, description, action }: EmptyStateProps) {
  return (
    <div className="flex flex-col items-center px-6 py-16 text-center">
      <span className="mb-4 flex size-11 items-center justify-center rounded-xl bg-subtle text-muted ring-1 ring-line">
        <Icon size={20} />
      </span>
      <h2 className="text-sm font-medium">{title}</h2>
      <p className="mt-1 max-w-xs text-sm text-muted">{description}</p>
      {action && <div className="mt-5">{action}</div>}
    </div>
  );
}
