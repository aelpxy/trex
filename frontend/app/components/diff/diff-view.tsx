import { useMemo } from "react";

import { diffRows } from "./diff";

const ROW_STYLE = {
  add: { row: "bg-diff-add", sign: "+", signClass: "text-diff-add-fg", label: "added" },
  remove: { row: "bg-diff-remove", sign: "−", signClass: "text-diff-remove-fg", label: "removed" },
  context: { row: "", sign: "", signClass: "", label: "" },
};

type DiffViewProps = { path: string; before: string; after: string; className?: string };

export function DiffView({ path, before, after, className = "" }: DiffViewProps) {
  const rows = useMemo(() => diffRows(path, before, after), [path, before, after]);

  if (rows.length === 0) return <p className="p-4 text-xs text-muted">No changes.</p>;

  return (
    <div className={`overflow-auto ${className}`}>
      <table className="w-full border-collapse font-mono text-[11px] leading-5">
        <caption className="sr-only">Changes to {path}</caption>
        <tbody>
          {rows.map((row, index) =>
            row.kind === "hunk" ? (
              <tr key={index} className="bg-subtle/60 text-muted">
                <td colSpan={4} className="px-3 py-1 select-none">
                  {row.label}
                </td>
              </tr>
            ) : (
              <tr key={index} className={ROW_STYLE[row.kind].row}>
                <td className="w-10 px-2 text-right text-muted/70 tabular-nums select-none">{row.oldLine}</td>
                <td className="w-10 px-2 text-right text-muted/70 tabular-nums select-none">{row.newLine}</td>
                <td className={`w-4 text-center select-none ${ROW_STYLE[row.kind].signClass}`}>
                  <span aria-hidden>{ROW_STYLE[row.kind].sign}</span>
                  {ROW_STYLE[row.kind].label && <span className="sr-only">{ROW_STYLE[row.kind].label}</span>}
                </td>
                <td className="pr-4 whitespace-pre">{row.text}</td>
              </tr>
            ),
          )}
        </tbody>
      </table>
    </div>
  );
}
