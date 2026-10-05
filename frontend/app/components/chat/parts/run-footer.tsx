import { Tooltip } from "@base-ui/react/tooltip";
import { LuBan, LuClock, LuCoins, LuGauge, LuInfo } from "react-icons/lu";

import { focusRing, tooltip } from "~/components/ui/styles";
import { formatUsd } from "~/lib/credits";

import type { AssistantMessage, Usage } from "../types";
import { formatDuration, formatTokens } from "./format";

const number = (count: number) => count.toLocaleString();
const seconds = (ms: number) => `${(ms / 1000).toFixed(ms < 10_000 ? 2 : 1)}s`;

function Row({ label, value, note }: { label: string; value: string; note?: string }) {
  return (
    <div className="flex items-baseline justify-between gap-6">
      <dt className="text-muted">{label}</dt>
      <dd className="text-right tabular-nums">
        {value}
        {note && <span className="ml-1 text-muted">{note}</span>}
      </dd>
    </div>
  );
}

// everything the turn's model requests reported, summed over its responses
function UsageDetails({ usage, workedMs }: { usage: Usage; workedMs: number }) {
  const streamingMs = usage.modelMs - usage.firstTokenMs;
  const streamingSpeed = usage.firstTokenCount === usage.responses && streamingMs > 0 ? Math.round(usage.outputTokens / (streamingMs / 1000)) : null;
  return (
    <dl className="w-64 space-y-1 text-[11px] leading-4">
      {usage.models.length > 0 && <Row label={usage.models.length === 1 ? "Model" : "Models"} value={usage.models.join(", ")} />}
      <Row label="Input tokens" value={number(usage.inputTokens)} note={usage.cachedTokens ? `${number(usage.cachedTokens)} cached` : undefined} />
      {usage.cacheWriteTokens > 0 && <Row label="Cache writes" value={number(usage.cacheWriteTokens)} />}
      <Row label="Output tokens" value={number(usage.outputTokens)} note={usage.reasoningTokens ? `${number(usage.reasoningTokens)} reasoning` : undefined} />
      <Row label="Largest request" value={number(usage.peakInputTokens)} note="tokens" />
      <Row label="Model responses" value={number(usage.responses)} />
      <div className="my-1.5 h-px bg-line" />
      <Row label="Worked for" value={seconds(workedMs)} />
      <Row label="Model time" value={seconds(usage.modelMs)} />
      {usage.firstTokenCount > 0 && <Row label="First token" value={seconds(usage.firstTokenMs / usage.firstTokenCount)} note="avg" />}
      {usage.modelMs > 0 && <Row label="Speed" value={`${Math.round(usage.outputTokens / (usage.modelMs / 1000))} tok/s`} note="incl. wait" />}
      {streamingSpeed !== null && <Row label="Streaming speed" value={`${streamingSpeed} tok/s`} />}
      {usage.credits > 0 && (
        <>
          <div className="my-1.5 h-px bg-line" />
          <Row label="Cost" value={formatUsd(usage.credits)} />
        </>
      )}
    </dl>
  );
}

function UsageInfo({ usage, workedMs }: { usage: Usage; workedMs: number }) {
  return (
    <Tooltip.Root>
      <Tooltip.Trigger
        render={<button type="button" />}
        aria-label="Usage details"
        className={`inline-flex size-5 cursor-help items-center justify-center rounded text-muted transition-colors hover:text-ink ${focusRing}`}
      >
        <LuInfo size={12} />
      </Tooltip.Trigger>
      <Tooltip.Portal>
        <Tooltip.Positioner side="top" sideOffset={8}>
          <Tooltip.Popup className={`px-3 py-2.5 ${tooltip}`}>
            <UsageDetails usage={usage} workedMs={workedMs} />
          </Tooltip.Popup>
        </Tooltip.Positioner>
      </Tooltip.Portal>
    </Tooltip.Root>
  );
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
        <span className="flex items-center gap-1 tabular-nums">
          <LuCoins size={12} />
          {formatTokens(tokens)} tokens{usage.credits > 0 && ` · ${formatUsd(usage.credits)}`}
        </span>
      )}
      {speed !== null && (
        <span className="flex items-center gap-1 tabular-nums" title="Output tokens per second while the model was generating">
          <LuGauge size={12} />
          {speed} tok/s
        </span>
      )}
      {usage && <UsageInfo usage={usage} workedMs={message.endedAt - message.startedAt} />}
    </p>
  );
}
