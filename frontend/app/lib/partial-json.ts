const ESCAPES: Record<string, string> = { '"': '"', "\\": "\\", "/": "/", b: "\b", f: "\f", n: "\n", r: "\r", t: "\t" };

// the value of a string field in JSON that is still being written, as far as it has arrived; `complete` waits for all of it
export function partialString(json: string, key: string, complete = false): string | undefined {
  const start = new RegExp(`"${key}"\\s*:\\s*"`).exec(json);
  if (!start) return undefined;
  let value = "";
  for (let at = start.index + start[0].length; at < json.length; at++) {
    const char = json[at];
    if (char === '"') return value;
    if (char !== "\\") {
      value += char;
      continue;
    }
    const escape = json[at + 1];
    if (escape === undefined) return complete ? undefined : value;
    if (escape === "u") {
      const hex = json.slice(at + 2, at + 6);
      if (hex.length < 4) return complete ? undefined : value;
      value += String.fromCharCode(parseInt(hex, 16));
      at += 5;
    } else {
      value += ESCAPES[escape] ?? escape;
      at += 1;
    }
  }
  return complete ? undefined : value;
}
