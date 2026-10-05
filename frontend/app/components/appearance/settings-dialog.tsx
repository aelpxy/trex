import { useRef, useState, type ChangeEvent } from "react";
import { Dialog } from "@base-ui/react/dialog";
import { Slider } from "@base-ui/react/slider";

import { Button } from "~/components/ui/button";
import { backdrop, dialogDescription, dialogPopup, dialogTitle, dialogViewport } from "~/components/ui/styles";

import { useAppearance } from "./appearance-provider";
import { DEFAULT_SETTINGS } from "./storage";

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

export function SettingsDialog() {
  const { settings, setSettings, backgroundUrl, setBackground, settingsOpen, setSettingsOpen } = useAppearance();
  const [error, setError] = useState<string | null>(null);
  const fileInput = useRef<HTMLInputElement>(null);

  async function updateBackground(image: File | null) {
    setError(null);
    try {
      await setBackground(image);
    } catch (cause) {
      console.warn("could not update background image", cause);
      setError(image ? "Couldn't save that image." : "Couldn't remove the image.");
    }
  }

  function chooseImage(event: ChangeEvent<HTMLInputElement>) {
    const file = event.target.files?.[0];
    event.target.value = "";
    if (!file) return;
    if (!file.type.startsWith("image/")) {
      setError("That file isn't an image.");
    } else if (file.size > MAX_IMAGE_BYTES) {
      setError("Images must be 20 MB or smaller.");
    } else {
      void updateBackground(file);
    }
  }

  return (
    <Dialog.Root open={settingsOpen} onOpenChange={setSettingsOpen}>
      <Dialog.Portal>
        <Dialog.Backdrop className={backdrop} />
        <Dialog.Viewport className={dialogViewport}>
          <Dialog.Popup className={`${dialogPopup} max-w-md`}>
            <Dialog.Title className={dialogTitle}>Appearance</Dialog.Title>
            <Dialog.Description className={dialogDescription}>Set a background image and tune the glass effect.</Dialog.Description>

            <div className="mt-5">
              <p className="mb-1.5 text-xs font-medium text-muted">Background</p>
              <div className="flex aspect-video items-center justify-center overflow-hidden rounded-lg bg-subtle bg-cover bg-center ring-1 ring-line" style={backgroundUrl ? { backgroundImage: `url("${backgroundUrl}")` } : undefined}>
                {!backgroundUrl && <span className="text-xs text-muted">No image</span>}
              </div>
              <input ref={fileInput} type="file" accept="image/*" onChange={chooseImage} className="hidden" />
              <div className="mt-3 flex gap-2">
                <Button onClick={() => fileInput.current?.click()}>Choose image</Button>
                <Button variant="quiet" onClick={() => void updateBackground(null)} disabled={!backgroundUrl}>
                  Remove
                </Button>
              </div>
              {error && <p role="alert" className="mt-2 text-xs text-danger">{error}</p>}
            </div>

            <div className="mt-5 space-y-2">
              <SettingSlider label="Blur" value={settings.blur} min={0} max={40} unit="px" onChange={(blur) => setSettings({ ...settings, blur })} />
              <SettingSlider label="Glass opacity" value={settings.opacity} min={20} max={100} unit="%" onChange={(opacity) => setSettings({ ...settings, opacity })} />
            </div>

            <div className="mt-6 flex justify-between gap-2">
              <Button variant="quiet" onClick={() => setSettings(DEFAULT_SETTINGS)}>
                Reset
              </Button>
              <Dialog.Close render={<Button />}>Done</Dialog.Close>
            </div>
          </Dialog.Popup>
        </Dialog.Viewport>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
