import { useState, type KeyboardEvent } from "react";

import { focusRing } from "~/components/ui/styles";

import { normalizePath } from "./files-provider";

type PathInputProps = {
  label: string;
  initial?: string;
  validate: (path: string) => string | null;
  onSubmit: (path: string) => void;
  onCancel: () => void;
};

export function PathInput({ label, initial = "", validate, onSubmit, onCancel }: PathInputProps) {
  const [value, setValue] = useState(initial);
  const [error, setError] = useState<string | null>(null);

  function onKeyDown(event: KeyboardEvent<HTMLInputElement>) {
    if (event.key === "Escape") {
      event.stopPropagation();
      onCancel();
    }
    if (event.key !== "Enter") return;
    const path = normalizePath(value);
    const problem = path ? validate(path) : "Enter a file path.";
    if (problem) setError(problem);
    else onSubmit(path);
  }

  return (
    <div className="px-2 py-1">
      <input
        autoFocus
        value={value}
        aria-label={label}
        aria-invalid={error !== null}
        placeholder="folder/file.py"
        onChange={(event) => {
          setValue(event.target.value);
          setError(null);
        }}
        onFocus={(event) => {
          const base = event.currentTarget.value.lastIndexOf("/") + 1;
          const extension = event.currentTarget.value.lastIndexOf(".");
          event.currentTarget.setSelectionRange(base, extension > base ? extension : event.currentTarget.value.length);
        }}
        onKeyDown={onKeyDown}
        onBlur={onCancel}
        className={`h-7 w-full rounded-md border border-line bg-surface px-2 font-mono text-[11px] ${focusRing}`}
      />
      {error && (
        <p role="alert" className="mt-1 text-[11px] text-danger">
          {error}
        </p>
      )}
    </div>
  );
}
