import { useEffect, useState } from "react";
import { Input } from "@base-ui/react/input";

import { focusRing } from "./styles";

const SETTLE_MS = 250;

// typing stays local until it pauses, so a search the server runs doesn't fire on every key
export function FilterInput({ value, onChange, label }: { value: string; onChange: (value: string) => void; label: string }) {
  const [draft, setDraft] = useState(value);
  const [synced, setSynced] = useState(value);
  // follows the url when it changes elsewhere, like the back button
  if (value !== synced) {
    setSynced(value);
    setDraft(value);
  }

  useEffect(() => {
    if (draft === value) return;
    const timer = setTimeout(() => onChange(draft), SETTLE_MS);
    return () => clearTimeout(timer);
  }, [draft, value, onChange]);

  return <Input type="search" value={draft} onValueChange={setDraft} aria-label={label} placeholder={label} className={`ui-input h-10 ${focusRing}`} />;
}
