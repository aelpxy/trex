import { useRef, type ChangeEvent } from "react";
import { Slider } from "@base-ui/react/slider";

import { Section } from "~/components/account/section";
import { Button } from "~/components/ui/button";
import { Switch } from "~/components/ui/switch";
import { toasts } from "~/lib/toasts";

import { useAppearance } from "./appearance-provider";
import { DEFAULT_SETTINGS } from "./storage";
import { useTheme } from "./use-theme";

const MAX_IMAGE_BYTES = 20 * 1024 * 1024;

type SettingSliderProps = { label: string; value: number; min: number; max: number; unit: string; onChange: (value: number) => void };

function SettingSlider({ label, value, min, max, unit, onChange }: SettingSliderProps) {
  return (
    <Slider.Root value={value} min={min} max={max} onValueChange={(next) => onChange(next as number)}>
      <div className="flex items-center justify-between text-xs">
        <Slider.Label className="font-medium text-muted">{label}</Slider.Label>
        <Slider.Value className="font-mono text-muted">{(_, values) => `${values[0]}${unit}`}</Slider.Value>
      </div>
      <Slider.Control className="flex w-full touch-none items-center py-3 select-none">
        <Slider.Track className="h-1 w-full rounded-full bg-line select-none">
          <Slider.Indicator className="rounded-full bg-ink select-none" />
          <Slider.Thumb className="size-4 cursor-grab rounded-full border border-line bg-surface shadow-sm select-none active:cursor-grabbing has-focus-visible:outline-2 has-focus-visible:outline-offset-2 has-focus-visible:outline-accent" />
        </Slider.Track>
      </Slider.Control>
    </Slider.Root>
  );
}

export function AppearanceSettings() {
  const { settings, setSettings, backgroundUrl, setBackground } = useAppearance();
  const { theme, setTheme } = useTheme();
  const fileInput = useRef<HTMLInputElement>(null);

  async function updateBackground(image: File | null) {
    try {
      await setBackground(image);
      toasts.add({ title: image ? "Background updated" : "Background removed", type: "success" });
    } catch (cause) {
      console.warn("could not update background image", cause);
      toasts.add({ title: image ? "Couldn't save that image" : "Couldn't remove the image", type: "error" });
    }
  }

  function chooseImage(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (!file.type.startsWith("image/")) {
      toasts.add({ title: "That file isn't an image", type: "error" });
    } else if (file.size > MAX_IMAGE_BYTES) {
      toasts.add({ title: "That image is too large", description: "Images must be 20 MB or smaller.", type: "error" });
    } else {
      void updateBackground(file);
    }
  }

  return (
    <>
      <Section title="Theme">
        <div className="ui-card px-5 py-4">
          <Switch checked={theme === "dark"} onCheckedChange={(dark) => setTheme(dark ? "dark" : "light")} description="Easier on the eyes at night. Saved in this browser.">
            Dark mode
          </Switch>
        </div>
      </Section>
      <Section title="Background" description="An image behind the app, kept in this browser only.">
        <div className="ui-card px-5 py-4">
          <div className="flex aspect-video items-center justify-center overflow-hidden rounded-lg bg-subtle bg-cover bg-center ring-1 ring-line" style={backgroundUrl ? { backgroundImage: `url("${backgroundUrl}")` } : undefined}>
            {!backgroundUrl && <span className="text-xs text-muted">No image</span>}
          </div>
          <input ref={fileInput} type="file" accept="image/*" onChange={chooseImage} className="hidden" />
          <div className="mt-3 flex gap-2">
            <Button onClick={() => fileInput.current?.click()}>{backgroundUrl ? "Change image" : "Choose image"}</Button>
            <Button variant="quiet" onClick={() => void updateBackground(null)} disabled={!backgroundUrl}>
              Remove
            </Button>
          </div>
        </div>
      </Section>
      <Section title="Glass" description="How much the panels blur and let the background through.">
        <div className="ui-card space-y-2 px-5 py-4">
          <SettingSlider label="Blur" value={settings.blur} min={0} max={40} unit="px" onChange={(blur) => setSettings({ ...settings, blur })} />
          <SettingSlider label="Glass opacity" value={settings.opacity} min={20} max={100} unit="%" onChange={(opacity) => setSettings({ ...settings, opacity })} />
          <div className="flex justify-end pt-1">
            <Button variant="quiet" onClick={() => setSettings(DEFAULT_SETTINGS)} size="sm">
              Reset to defaults
            </Button>
          </div>
        </div>
      </Section>
    </>
  );
}
