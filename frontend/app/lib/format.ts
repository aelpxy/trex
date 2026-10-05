// how numbers, counts and times read across the app

export const count = (value: number) => value.toLocaleString();

// `3 files`, `1 file`; irregular plurals pass their own
export const plural = (value: number, one: string, many = `${one}s`) => `${count(value)} ${value === 1 ? one : many}`;

export const tokens = (value: number) =>
  value >= 1_000_000 ? `${(value / 1_000_000).toFixed(1)}M` : value >= 1000 ? `${(value / 1000).toFixed(1)}k` : String(value);

export const date = (seconds: number) => new Date(seconds * 1000).toLocaleDateString(undefined, { dateStyle: "medium" });

export const dateTime = (seconds: number) => new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short" });

export function ago(seconds: number | null) {
  if (seconds === null) return "Never";
  const minutes = Math.round((Date.now() / 1000 - seconds) / 60);
  if (minutes < 1) return "Just now";
  if (minutes < 60) return `${minutes}m ago`;
  if (minutes < 60 * 24) return `${Math.round(minutes / 60)}h ago`;
  return `${Math.round(minutes / 60 / 24)}d ago`;
}

export const errorText = (cause: unknown) => (cause instanceof Error ? cause.message : String(cause));
