import { Link, useSearchParams } from "react-router";
import { LuChevronLeft, LuChevronRight } from "react-icons/lu";

import { focusRing } from "./styles";

// pages either side of the current one before the list collapses into an ellipsis
const NEIGHBOURS = 1;

// 1 … 4 5 6 … 9: the first, last and nearby pages, with gaps where pages are skipped
export function pageList(page: number, pages: number): (number | "gap")[] {
  const shown = new Set([1, pages, ...Array.from({ length: NEIGHBOURS * 2 + 1 }, (_, index) => page - NEIGHBOURS + index)]);
  const sorted = [...shown].filter((candidate) => candidate >= 1 && candidate <= pages).sort((a, b) => a - b);
  return sorted.flatMap((candidate, index) => (index > 0 && candidate - sorted[index - 1] > 1 ? ["gap" as const, candidate] : [candidate]));
}

// the page number in the url, kept when the rest of the query changes
export function usePage(param = "page") {
  const [params] = useSearchParams();
  const page = Number(params.get(param));
  return Number.isInteger(page) && page > 1 ? page : 1;
}

const link = `inline-flex h-8 min-w-8 items-center justify-center rounded-md px-2 text-xs tabular-nums text-muted transition-colors hover:bg-subtle hover:text-ink aria-[current=page]:bg-subtle aria-[current=page]:font-medium aria-[current=page]:text-ink ${focusRing}`;
const disabled = "pointer-events-none opacity-40";

type PaginationProps = { page: number; perPage: number; total: number; noun?: string; param?: string };

export function Pagination({ page, perPage, total, noun = "entries", param = "page" }: PaginationProps) {
  const [params] = useSearchParams();
  const pages = Math.max(1, Math.ceil(total / perPage));
  const href = (target: number) => {
    const next = new URLSearchParams(params);
    if (target > 1) next.set(param, String(target));
    else next.delete(param);
    const query = next.toString();
    return query ? `?${query}` : "?";
  };
  const first = total === 0 ? 0 : (page - 1) * perPage + 1;
  const last = Math.min(page * perPage, total);

  return (
    <nav aria-label="Pages" className="mt-3 flex flex-wrap items-center justify-between gap-3">
      <p className="text-xs text-muted tabular-nums">
        {total === 0 ? `No ${noun}` : `Showing ${first.toLocaleString()}–${last.toLocaleString()} of ${total.toLocaleString()} ${noun}`}
      </p>
      {pages > 1 && (
        <div className="flex items-center gap-1">
          <Link to={href(page - 1)} preventScrollReset aria-label="Previous page" aria-disabled={page <= 1} className={`${link} ${page <= 1 ? disabled : ""}`}>
            <LuChevronLeft size={14} />
          </Link>
          {pageList(page, pages).map((entry, index) =>
            entry === "gap" ? (
              <span key={`gap-${index}`} aria-hidden className="px-1 text-xs text-muted">
                …
              </span>
            ) : (
              <Link key={entry} to={href(entry)} preventScrollReset aria-current={entry === page ? "page" : undefined} aria-label={`Page ${entry}`} className={link}>
                {entry}
              </Link>
            ),
          )}
          <Link to={href(page + 1)} preventScrollReset aria-label="Next page" aria-disabled={page >= pages} className={`${link} ${page >= pages ? disabled : ""}`}>
            <LuChevronRight size={14} />
          </Link>
        </div>
      )}
    </nav>
  );
}
