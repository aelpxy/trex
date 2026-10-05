import type { ReactNode } from "react";

export function Section({ title, description, children }: { title: string; description?: string; children: ReactNode }) {
  return (
    <section className="mt-8 first:mt-0">
      <h2 className="text-sm font-medium">{title}</h2>
      {description && <p className="mt-1 text-xs text-muted">{description}</p>}
      <div className="mt-4">{children}</div>
    </section>
  );
}

// how the last change went: its error, or a note once it succeeded
export function Status({ error, saved }: { error: unknown; saved: string | null }) {
  if (error) return <p role="alert" className="text-xs text-danger">{error instanceof Error ? error.message : String(error)}</p>;
  if (saved) return <p role="status" className="text-xs text-muted">{saved}</p>;
  return null;
}
