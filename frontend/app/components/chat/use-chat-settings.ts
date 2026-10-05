import { useEffect, useState } from "react";

import { DEFAULT_CHAT_SETTINGS, effortFor, MODELS, type ChatSettings } from "./models";

const SETTINGS_KEY = "trex-chat-settings";

function load(): ChatSettings {
  try {
    const parsed = JSON.parse(localStorage.getItem(SETTINGS_KEY) ?? "null");
    if (!parsed) return DEFAULT_CHAT_SETTINGS;
    const model = MODELS.some((option) => option.value === parsed.model) ? parsed.model : DEFAULT_CHAT_SETTINGS.model;
    return {
      model,
      effort: effortFor(model, typeof parsed.effort === "string" ? parsed.effort : DEFAULT_CHAT_SETTINGS.effort),
      fast: parsed.fast === true,
    };
  } catch (error) {
    console.warn("could not read chat settings", error);
    return DEFAULT_CHAT_SETTINGS;
  }
}

export function useChatSettings() {
  const [settings, setSettings] = useState(DEFAULT_CHAT_SETTINGS);
  const [loaded, setLoaded] = useState(false);

  useEffect(() => {
    setSettings(load());
    setLoaded(true);
  }, []);

  useEffect(() => {
    if (!loaded) return;
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    } catch (error) {
      console.warn("could not save chat settings", error);
    }
  }, [settings, loaded]);

  // a new model may not offer the current effort level
  const update = (patch: Partial<ChatSettings>) =>
    setSettings((current) => {
      const next = { ...current, ...patch };
      return { ...next, effort: effortFor(next.model, next.effort) };
    });

  return { settings, update };
}
