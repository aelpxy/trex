import { errorText } from "./format";

// a list or panel that couldn't load; action outcomes go in toasts instead
export function LoadError({ error }: { error: unknown }) {
  if (!error) return null;
  return (
    <p role="alert" className="text-xs text-danger">
      {errorText(error)}
    </p>
  );
}
