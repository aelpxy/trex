import { errorText } from "./format";

// what happened to the last change: its error, or a note once it succeeded
export function ActionStatus({ error, success }: { error: unknown; success: string | null }) {
  if (error) return <p role="alert" className="text-xs text-danger">{errorText(error)}</p>;
  if (success) return <p role="status" className="text-xs text-muted">{success}</p>;
  return null;
}
