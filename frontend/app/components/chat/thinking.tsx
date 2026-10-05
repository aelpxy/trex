import { LuSparkles } from "react-icons/lu";

export function Thinking() {
  return (
    <p role="status" className="flex items-center gap-2 text-sm text-muted">
      <LuSparkles size={15} className="animate-[think-pulse_1.6s_ease-in-out_infinite]" />
      <span className="thinking-label">Thinking</span>
      <span aria-hidden className="flex gap-0.5">
        {[0, 1, 2].map((dot) => (
          <span key={dot} className="size-1 animate-[think-dot_1.2s_ease-in-out_infinite] rounded-full bg-muted" style={{ animationDelay: `${dot * 0.15}s` }} />
        ))}
      </span>
    </p>
  );
}
