import { LuBan, LuClock, LuCoins, LuGauge } from "react-icons/lu";

import type { AssistantMessage, Usage } from "../types";
import { formatDuration, formatTokens } from "./format";

function details(usage: Usage) {
  const lines = [
    `${usage.inputTokens.toLocaleString()} input${usage.cachedTokens ? ` (${usage.cachedTokens.toLocaleString()} cached)` : ""}`,
    `${usage.outputTokens.toLocaleString()} output${usage.reasoningTokens ? ` (${usage.reasoningTokens.toLocaleString()} reasoning)` : ""}`,
    `${usage.responses} model ${usage.responses === 1 ? "response" : "responses"}`,
  ];
  if (usage.credits) lines.push(`${usage.credits.toLocaleString()} ${usage.credits === 1 ? "credit" : "credits"}`);
  return lines.join("\n");
}

export function RunFooter({ message }: { message: AssistantMessage }) {
  if (!message.endedAt) return null;
  const usage = message.usage;
  const tokens = usage ? usage.inputTokens + usage.outputTokens : null;
  // generation speed, from the time the model spent producing output
  const speed = usage && usage.modelMs > 0 ? Math.round(usage.outputTokens / (usage.modelMs / 1000)) : null;

  return (
    <p className="flex items-center gap-3 text-[11px] text-muted">
      {message.state === "cancelled" && (
        <span className="flex items-center gap-1">
          <LuBan size={12} />
          Stopped
        </span>
      )}
      <span className="flex items-center gap-1 tabular-nums">
        <LuClock size={12} />
        Worked for {formatDuration(message.endedAt - message.startedAt)}
      </span>
      {usage && tokens !== null && (
        <span className="flex items-center gap-1 tabular-nums" title={details(usage)}>
          <LuCoins size={12} />
          {formatTokens(tokens)} tokens{usage.credits > 0 && ` · ${usage.credits.toLocaleString()} ${usage.credits === 1 ? "credit" : "credits"}`}
        </span>
      )}
      {speed !== null && (
        <span className="flex items-center gap-1 tabular-nums" title="Output tokens per second while the model was generating">
          <LuGauge size={12} />
          {speed} tok/s
        </span>
      )}
    </p>
  );
}
