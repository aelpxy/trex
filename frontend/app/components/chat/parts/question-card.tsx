import { useState, type FormEvent } from "react";
import { Button as BaseButton } from "@base-ui/react/button";
import { LuMessageCircleQuestion } from "react-icons/lu";

import { Button } from "~/components/ui/button";
import { focusRing, focusRingOutset } from "~/components/ui/styles";

import type { QuestionPart } from "../types";

type QuestionCardProps = { part: QuestionPart; onAnswer: (answer: string) => void };

export function QuestionCard({ part, onAnswer }: QuestionCardProps) {
  const [other, setOther] = useState("");

  if (part.answer !== undefined) {
    return (
      <p className="flex items-center gap-2 text-xs text-muted">
        <LuMessageCircleQuestion size={13} />
        {part.question} <span className="font-medium text-ink">{part.answer}</span>
      </p>
    );
  }

  function submitOther(event: FormEvent<HTMLFormElement>) {
    event.preventDefault();
    if (other.trim()) onAnswer(other.trim());
  }

  return (
    <section aria-label="Question from the agent" className="rounded-xl border border-line bg-surface/60 p-4">
      <div className="flex gap-3">
        <LuMessageCircleQuestion size={18} className="mt-0.5 shrink-0 text-muted" />
        <h3 className="text-sm font-medium">{part.question}</h3>
      </div>
      <div className="mt-3 flex flex-wrap gap-2 pl-7.5">
        {part.options.map((option) => (
          <BaseButton
            key={option}
            onClick={() => onAnswer(option)}
            className={`h-8 cursor-pointer rounded-md border border-line px-3 text-[13px] transition-colors hover:bg-subtle ${focusRingOutset}`}
          >
            {option}
          </BaseButton>
        ))}
      </div>
      <form onSubmit={submitOther} className="mt-3 flex gap-2 pl-7.5">
        <input
          value={other}
          onChange={(event) => setOther(event.target.value)}
          aria-label="Other answer"
          placeholder="Or type an answer"
          className={`h-9 min-w-0 flex-1 rounded-md border border-line bg-surface px-3 text-[13px] placeholder:text-muted ${focusRing}`}
        />
        <Button type="submit" variant="quiet" disabled={!other.trim()}>
          Send
        </Button>
      </form>
    </section>
  );
}
