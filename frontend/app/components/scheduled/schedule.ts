import cronstrue from "cronstrue";

export type Frequency = "hourly" | "daily" | "weekdays" | "weekly" | "monthly" | "custom";

// a schedule as the form edits it; `cron` is only used for custom schedules
export type ScheduleParts = { frequency: Frequency; minute: number; hour: number; weekday: number; day: number; cron: string };

export const FREQUENCIES: { value: Frequency; label: string }[] = [
  { value: "hourly", label: "Every hour" },
  { value: "daily", label: "Every day" },
  { value: "weekdays", label: "Every weekday" },
  { value: "weekly", label: "Every week" },
  { value: "monthly", label: "Every month" },
  { value: "custom", label: "Custom (cron)" },
];

export const WEEKDAYS = ["Sunday", "Monday", "Tuesday", "Wednesday", "Thursday", "Friday", "Saturday"];

export const DEFAULT_PARTS: ScheduleParts = { frequency: "daily", minute: 0, hour: 9, weekday: 1, day: 1, cron: "0 9 * * *" };

export function toCron(parts: ScheduleParts): string {
  const { minute, hour } = parts;
  switch (parts.frequency) {
    case "hourly":
      return `${minute} * * * *`;
    case "daily":
      return `${minute} ${hour} * * *`;
    case "weekdays":
      return `${minute} ${hour} * * 1-5`;
    case "weekly":
      return `${minute} ${hour} * * ${parts.weekday}`;
    case "monthly":
      return `${minute} ${hour} ${parts.day} * *`;
    case "custom":
      return parts.cron.trim();
  }
}

// reads a stored schedule back into the form; anything the presets don't cover stays custom
export function fromCron(cron: string): ScheduleParts {
  const fields = cron.trim().split(/\s+/);
  const number = (value: string | undefined) => (value !== undefined && /^\d+$/.test(value) ? Number(value) : null);
  const [minute, hour, day, month, weekday] = fields.map(number);
  const custom = { ...DEFAULT_PARTS, frequency: "custom" as const, cron };
  if (fields.length !== 5 || minute === null || fields[3] !== "*") return custom;
  const base = { ...DEFAULT_PARTS, minute, cron };
  if (fields[1] === "*" && fields[2] === "*" && fields[4] === "*") return { ...base, frequency: "hourly" };
  if (hour === null) return custom;
  if (fields[2] === "*" && fields[4] === "*") return { ...base, hour, frequency: "daily" };
  if (fields[2] === "*" && fields[4] === "1-5") return { ...base, hour, frequency: "weekdays" };
  if (fields[2] === "*" && weekday !== null && weekday <= 6) return { ...base, hour, weekday, frequency: "weekly" };
  if (day !== null && fields[4] === "*" && month === null) return { ...base, hour, day, frequency: "monthly" };
  return custom;
}

// "At 09:00, Monday through Friday"; the raw expression when it can't be read
export function describeSchedule(cron: string): string {
  try {
    return cronstrue.toString(cron, { use24HourTimeFormat: true, verbose: false });
  } catch {
    return cron;
  }
}

export const browserTimezone = () => Intl.DateTimeFormat().resolvedOptions().timeZone;

export const timezones = (): string[] => (typeof Intl.supportedValuesOf === "function" ? Intl.supportedValuesOf("timeZone") : [browserTimezone()]);

export function formatRunTime(seconds: number | null, timezone?: string) {
  if (seconds === null) return null;
  return new Date(seconds * 1000).toLocaleString(undefined, { dateStyle: "medium", timeStyle: "short", timeZone: timezone });
}
