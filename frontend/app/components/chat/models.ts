import type { IconType } from "react-icons";
import { LuFlame, LuSignal, LuSignalHigh, LuSignalLow, LuSignalMedium, LuSignalZero } from "react-icons/lu";

import type { ApiModel } from "~/lib/trex";

export type Option<T extends string> = { value: T; label: string; description: string; icon?: IconType };

// filled from GET /v1/models by the layout's loader before the app renders
export const MODELS: Option<string>[] = [];

const API_MODELS = new Map<string, ApiModel>();

// the levels a model accepts come from GET /v1/models; these describe the ones trex knows
export type Effort = string;

const EFFORT_INFO: Record<string, Omit<Option<Effort>, "value">> = {
  none: { label: "None", description: "Answers right away, no thinking", icon: LuSignalZero },
  minimal: { label: "Minimal", description: "Barely thinks before answering", icon: LuSignalZero },
  low: { label: "Low", description: "Quick answers, minimal thinking", icon: LuSignalLow },
  medium: { label: "Medium", description: "Balanced speed and depth", icon: LuSignalMedium },
  high: { label: "High", description: "Thinks longest on hard problems", icon: LuSignalHigh },
  xhigh: { label: "Extra high", description: "Even more thinking for very hard problems", icon: LuSignal },
  max: { label: "Max", description: "Thinks as long as it needs, slowest", icon: LuFlame },
};

const effortOption = (value: string): Option<Effort> => ({ value, ...(EFFORT_INFO[value] ?? { label: value.charAt(0).toUpperCase() + value.slice(1), description: `${value} reasoning effort` }) });

// models that don't list their levels accept any, so they get the common three
const DEFAULT_EFFORTS = ["low", "medium", "high"];

export const EFFORTS: Option<Effort>[] = DEFAULT_EFFORTS.map(effortOption);

export function effortsFor(model: string): Option<Effort>[] {
  const levels = API_MODELS.get(model)?.reasoning_efforts;
  return levels?.length ? levels.map(effortOption) : EFFORTS;
}

// keeps the chosen level when the model has it, otherwise the closest sensible default
export function effortFor(model: string, effort: Effort): Effort {
  const levels = effortsFor(model).map((option) => option.value);
  if (levels.includes(effort)) return effort;
  return levels.includes("medium") ? "medium" : levels[0];
}

export const contextWindowOf = (model: string) => API_MODELS.get(model)?.context_window ?? null;

export const supportsFast = (model: string) => API_MODELS.get(model)?.fast === true;

export type ChatSettings = { model: string; effort: Effort; fast: boolean };

export const DEFAULT_CHAT_SETTINGS: ChatSettings = { model: "", effort: "medium", fast: false };

function describe(model: ApiModel) {
  const window = model.context_window >= 1_000_000 ? `${(model.context_window / 1_000_000).toFixed(1).replace(/\.0$/, "")}M` : `${Math.round(model.context_window / 1000)}k`;
  return `${window} context${model.fast ? ", fast mode" : ""}`;
}

export function setModels(models: ApiModel[]) {
  API_MODELS.clear();
  models.forEach((model) => API_MODELS.set(model.id, model));
  MODELS.splice(0, MODELS.length, ...models.map((model) => ({ value: model.id, label: model.name, description: describe(model) })));
  DEFAULT_CHAT_SETTINGS.model = MODELS[0]?.value ?? "";
}

// what a session is created with: an effort the model doesn't list falls back to the provider default,
// and fast only where the model offers it
export function sessionSettings(settings: ChatSettings) {
  const model = API_MODELS.get(settings.model) ?? API_MODELS.values().next().value;
  const efforts = model?.reasoning_efforts;
  return {
    model: model?.id ?? settings.model,
    reasoning_effort: !efforts || efforts.includes(settings.effort) ? settings.effort : null,
    fast: settings.fast && model?.fast === true,
  };
}
