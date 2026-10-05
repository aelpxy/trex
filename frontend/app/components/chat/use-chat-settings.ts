import { useEffect, useState } from "react";

import { trex, type ApiSession } from "~/lib/trex";

import { DEFAULT_CHAT_SETTINGS, effortFor, MODELS, sessionSettings, type ChatSettings } from "./models";

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

const settingsOf = (session: ApiSession): ChatSettings => ({
  model: session.model,
  effort: effortFor(session.model, session.reasoning_effort ?? DEFAULT_CHAT_SETTINGS.effort),
  fast: session.fast,
});

// an open chat uses and updates its session's settings; a new chat starts from the last ones picked
export function useChatSettings(session?: ApiSession) {
  const [settings, setSettings] = useState(() => (session ? settingsOf(session) : DEFAULT_CHAT_SETTINGS));
  const [loaded, setLoaded] = useState(false);
  const sessionId = session?.id;

  useEffect(() => {
    if (!sessionId) setSettings(load());
    setLoaded(true);
  }, [sessionId]);

  useEffect(() => {
    if (!loaded) return;
    try {
      localStorage.setItem(SETTINGS_KEY, JSON.stringify(settings));
    } catch (error) {
      console.warn("could not save chat settings", error);
    }
  }, [settings, loaded]);

  // a new model may not offer the current effort level
  const update = (patch: Partial<ChatSettings>) => {
    const next = { ...settings, ...patch };
    const fitted = { ...next, effort: effortFor(next.model, next.effort) };
    setSettings(fitted);
    if (!sessionId) return;
    const { model, reasoning_effort, fast } = sessionSettings(fitted);
    trex.updateSession(sessionId, { model, reasoning_effort, fast }).catch((error) => {
      console.warn("could not change the chat's model", error);
      setSettings(settings);
    });
  };

  return { settings, update };
}
