import type { ReactNode } from "react";

export function Page({ title, children }: { title: string; children?: ReactNode }) {
  return (
    <div className="mx-auto w-full max-w-3xl px-6 py-16">
      <h1 className="text-2xl font-medium tracking-tight">{title}</h1>
      {children}
    </div>
  );
}
