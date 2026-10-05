import { useQuery } from "@tanstack/react-query";
import { useEffect, useRef, useState } from "react";
import { useSearchParams } from "react-router";

import { Button } from "~/components/ui/button";
import { focusRing } from "~/components/ui/styles";
import { queries } from "~/lib/queries";

import { ActionStatus } from "./action-status";
import { FilterInput } from "./filter-input";
import { matches, useUrlFilter } from "./use-url-filter";

const LEVELS = ["error", "warn", "info", "debug"] as const;
type Level = (typeof LEVELS)[number];
const LEVEL_LABEL: Record<Level, string> = { error: "Errors", warn: "Warnings", info: "Info", debug: "Debug" };
const RANK: Record<string, number> = { error: 0, warn: 1, info: 2, debug: 3, trace: 4 };
const LEVEL_STYLE: Record<string, string> = { error: "text-danger", warn: "text-ink", info: "text-muted", debug: "text-muted/70", trace: "text-muted/60" };
// how close to the bottom still counts as following the tail
const FOLLOW_SLACK_PX = 40;

const time = (ms: number) => new Date(ms).toLocaleTimeString(undefined, { hour12: false });

// a live tail of this server instance's own logs
export function LogTail() {
  const [paused, setPaused] = useState(false);
  const [params, setParams] = useSearchParams();
  const level: Level = LEVELS.find((option) => option === params.get("level")) ?? "info";
  const [filter, setFilter] = useUrlFilter();
  const logs = useQuery({ ...queries.admin.logs(), enabled: !paused });
  const scroller = useRef<HTMLDivElement>(null);
  const following = useRef(true);
  const shown = (logs.data ?? []).filter((line) => (RANK[line.level] ?? 4) <= RANK[level] && matches(filter, line.target, line.message, line.fields));

  useEffect(() => {
    const element = scroller.current;
    if (element && following.current) element.scrollTop = element.scrollHeight;
  }, [shown.length]);

  return (
    <div>
      <div className="flex flex-wrap items-center gap-2">
        <select
          value={level}
          onChange={(event) => setParams((current) => ({ ...Object.fromEntries(current), level: event.target.value }), { replace: true })}
          aria-label="Lowest level shown"
          className={`ui-input h-10 w-32 ${focusRing}`}
        >
          {LEVELS.map((option) => (
            <option key={option} value={option}>
              {LEVEL_LABEL[option]}
            </option>
          ))}
        </select>
        <div className="min-w-0 flex-1">
          <FilterInput value={filter} onChange={setFilter} label="Search logs" />
        </div>
        <Button variant="quiet" onClick={() => setPaused((current) => !current)} className="h-10 px-3 text-xs">
          {paused ? "Resume" : "Pause"}
        </Button>
      </div>
      <p className="mt-2 text-[11px] text-muted">This server instance's latest lines, refreshed every 2 seconds. Other instances keep their own.</p>
      <ActionStatus error={logs.error} success={null} />
      <div
        ref={scroller}
        onScroll={(event) => {
          const element = event.currentTarget;
          following.current = element.scrollHeight - element.scrollTop - element.clientHeight < FOLLOW_SLACK_PX;
        }}
        role="log"
        aria-label="Server logs"
        className="ui-card mt-3 h-[60vh] overflow-auto p-3 font-mono text-[11px] leading-5"
      >
        {shown.length === 0 && <p className="text-muted">{logs.isPending ? "Loading…" : "No lines match."}</p>}
        {shown.map((line) => (
          <div key={line.seq} className="whitespace-pre-wrap break-words">
            <span className="text-muted/70 select-none">{time(line.time)} </span>
            <span className={`uppercase ${LEVEL_STYLE[line.level] ?? "text-muted"}`}>{line.level.padEnd(5)} </span>
            <span className="text-muted">{line.target} </span>
            <span className="text-ink">{line.message}</span>
            {line.fields && <span className="text-muted"> {line.fields}</span>}
          </div>
        ))}
      </div>
    </div>
  );
}
