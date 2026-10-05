import { structuredPatch } from "diff";

export type DiffRow =
  | { kind: "hunk"; label: string }
  | { kind: "context" | "add" | "remove"; oldLine?: number; newLine?: number; text: string };

const CONTEXT_LINES = 3;

export function diffRows(path: string, before: string, after: string): DiffRow[] {
  const patch = structuredPatch(path, path, before, after, undefined, undefined, { context: CONTEXT_LINES });
  const rows: DiffRow[] = [];
  for (const hunk of patch.hunks) {
    rows.push({ kind: "hunk", label: `@@ -${hunk.oldStart},${hunk.oldLines} +${hunk.newStart},${hunk.newLines} @@` });
    let oldLine = hunk.oldStart;
    let newLine = hunk.newStart;
    for (const line of hunk.lines) {
      const text = line.slice(1);
      if (line.startsWith("+")) rows.push({ kind: "add", newLine: newLine++, text });
      else if (line.startsWith("-")) rows.push({ kind: "remove", oldLine: oldLine++, text });
      else if (line.startsWith(" ")) rows.push({ kind: "context", oldLine: oldLine++, newLine: newLine++, text });
    }
  }
  return rows;
}

export function diffStats(rows: DiffRow[]) {
  return {
    added: rows.filter((row) => row.kind === "add").length,
    removed: rows.filter((row) => row.kind === "remove").length,
  };
}
