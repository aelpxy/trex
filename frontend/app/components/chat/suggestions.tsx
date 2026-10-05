import { Button } from "@base-ui/react/button";
import { LuBug, LuFlaskConical, LuGlobe, LuTerminal } from "react-icons/lu";

import { focusRingOutset } from "~/components/ui/styles";

const SUGGESTIONS = [
  { icon: LuTerminal, label: "Set up a Python project", prompt: "Set up a new Python project with uv, pytest and a basic CLI." },
  { icon: LuBug, label: "Debug a failing test", prompt: "Help me debug a failing test. I'll paste the error." },
  { icon: LuGlobe, label: "Research a library", prompt: "Compare the most popular Rust HTTP clients and recommend one." },
  { icon: LuFlaskConical, label: "Analyze a dataset", prompt: "Load a CSV from my library and summarize the interesting patterns." },
];

export function Suggestions({ onPick }: { onPick: (prompt: string) => void }) {
  return (
    <ul aria-label="Suggestions" className="mt-4 flex flex-wrap justify-center gap-2">
      {SUGGESTIONS.map(({ icon: Icon, label, prompt }) => (
        <li key={label}>
          <Button
            onClick={() => onPick(prompt)}
            className={`glass inline-flex h-8 cursor-pointer items-center gap-2 rounded-full border border-line px-3 text-xs text-muted transition-colors hover:text-ink ${focusRingOutset}`}
          >
            <Icon size={13} />
            {label}
          </Button>
        </li>
      ))}
    </ul>
  );
}
