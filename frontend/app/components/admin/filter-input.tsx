import { focusRing } from "~/components/ui/styles";

export function FilterInput({ value, onChange, label }: { value: string; onChange: (value: string) => void; label: string }) {
  return <input type="search" value={value} onChange={(event) => onChange(event.target.value)} aria-label={label} placeholder={label} className={`ui-input h-10 ${focusRing}`} />;
}
