import { createContext, use, useCallback, useEffect, useMemo, useRef, useState, type ReactNode } from "react";

import { clearBackground, DEFAULT_SETTINGS, loadBackground, loadSettings, saveBackground, saveSettings, type GlassSettings } from "./storage";

type Appearance = {
  settings: GlassSettings;
  setSettings: (settings: GlassSettings) => void;
  backgroundUrl: string | null;
  setBackground: (image: File | null) => Promise<void>;
};

const AppearanceContext = createContext<Appearance | null>(null);

export function useAppearance() {
  const value = use(AppearanceContext);
  if (!value) throw new Error("useAppearance must be used inside AppearanceProvider");
  return value;
}

export function AppearanceProvider({ children }: { children: ReactNode }) {
  const [settings, setSettings] = useState(DEFAULT_SETTINGS);
  const [loaded, setLoaded] = useState(false);
  const [backgroundUrl, setBackgroundUrl] = useState<string | null>(null);
  const urlRef = useRef<string | null>(null);

  const showImage = useCallback((image: Blob | null) => {
    if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    urlRef.current = image ? URL.createObjectURL(image) : null;
    setBackgroundUrl(urlRef.current);
  }, []);

  useEffect(() => {
    setSettings(loadSettings());
    setLoaded(true);
    loadBackground()
      .then(showImage)
      .catch((error) => console.warn("could not load background image", error));
    return () => {
      if (urlRef.current) URL.revokeObjectURL(urlRef.current);
    };
  }, [showImage]);

  useEffect(() => {
    const root = document.documentElement.style;
    root.setProperty("--glass-blur", `${settings.blur}px`);
    root.setProperty("--glass-opacity", `${settings.opacity}%`);
    if (loaded) saveSettings(settings);
  }, [settings, loaded]);

  const setBackground = useCallback(
    async (image: File | null) => {
      await (image ? saveBackground(image) : clearBackground());
      showImage(image);
    },
    [showImage],
  );

  const value = useMemo(
    () => ({ settings, setSettings, backgroundUrl, setBackground }),
    [settings, backgroundUrl, setBackground],
  );

  return <AppearanceContext value={value}>{children}</AppearanceContext>;
}
