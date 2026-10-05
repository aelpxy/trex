export function formatDuration(ms: number) {
  const seconds = ms / 1000;
  if (seconds < 1) return "<1s";
  return seconds < 10 ? `${seconds.toFixed(1)}s` : `${Math.round(seconds)}s`;
}

export function formatTokens(count: number) {
  return count < 1000 ? String(count) : `${(count / 1000).toFixed(1)}k`;
}

const EXTENSION_LANGUAGES: Record<string, string> = { py: "python", ts: "ts", tsx: "tsx", js: "js", json: "json", toml: "toml", md: "markdown", sh: "bash", rs: "rust", go: "go" };

export const languageOf = (path = "") => EXTENSION_LANGUAGES[path.split(".").pop() ?? ""];
