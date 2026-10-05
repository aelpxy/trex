import { useState, type FormEvent, type ReactNode } from "react";
import { Dialog } from "@base-ui/react/dialog";
import { useMutation, useQueryClient } from "@tanstack/react-query";

import { DEFAULT_CHAT_SETTINGS, effortFor, effortsFor, MODELS } from "~/components/chat/models";
import { Button } from "~/components/ui/button";
import { backdrop, dialogDescription, dialogPopup, dialogTitle, dialogViewport, focusRing } from "~/components/ui/styles";
import { useWorkspace } from "~/components/workspace/workspace-provider";
import { queries } from "~/lib/queries";
import { trex, type ApiScheduledTask, type TaskInput } from "~/lib/trex";

import { browserTimezone, DEFAULT_PARTS, describeSchedule, FREQUENCIES, fromCron, timezones, toCron, WEEKDAYS, type Frequency, type ScheduleParts } from "./schedule";

const label = "mb-1.5 block text-xs font-medium text-muted";
const input = `ui-input h-10 ${focusRing}`;
const pad = (value: number) => String(value).padStart(2, "0");

function Field({ title, children }: { title: string; children: ReactNode }) {
  return (
    <label className="block">
      <span className={label}>{title}</span>
      {children}
    </label>
  );
}

function ScheduleFields({ parts, onChange }: { parts: ScheduleParts; onChange: (parts: ScheduleParts) => void }) {
  const set = (patch: Partial<ScheduleParts>) => onChange({ ...parts, ...patch });
  return (
    <div className="grid gap-3 sm:grid-cols-2">
      <Field title="Repeats">
        <select value={parts.frequency} onChange={(event) => set({ frequency: event.target.value as Frequency, cron: toCron(parts) })} className={input}>
          {FREQUENCIES.map((option) => (
            <option key={option.value} value={option.value}>
              {option.label}
            </option>
          ))}
        </select>
      </Field>
      {parts.frequency === "hourly" && (
        <Field title="Minute past the hour">
          <input type="number" min={0} max={59} value={parts.minute} onChange={(event) => set({ minute: Math.min(59, Math.max(0, Number(event.target.value))) })} className={input} />
        </Field>
      )}
      {parts.frequency !== "hourly" && parts.frequency !== "custom" && (
        <Field title="At">
          <input
            type="time"
            value={`${pad(parts.hour)}:${pad(parts.minute)}`}
            onChange={(event) => {
              const [hour, minute] = event.target.value.split(":").map(Number);
              if (Number.isFinite(hour) && Number.isFinite(minute)) set({ hour, minute });
            }}
            className={input}
          />
        </Field>
      )}
      {parts.frequency === "weekly" && (
        <Field title="On">
          <select value={parts.weekday} onChange={(event) => set({ weekday: Number(event.target.value) })} className={input}>
            {WEEKDAYS.map((day, index) => (
              <option key={day} value={index}>
                {day}
              </option>
            ))}
          </select>
        </Field>
      )}
      {parts.frequency === "monthly" && (
        <Field title="Day of the month">
          <input type="number" min={1} max={28} value={parts.day} onChange={(event) => set({ day: Math.min(28, Math.max(1, Number(event.target.value))) })} className={input} />
        </Field>
      )}
      {parts.frequency === "custom" && (
        <Field title="Cron expression">
          <input value={parts.cron} onChange={(event) => set({ cron: event.target.value })} placeholder="0 9 * * 1-5" spellCheck={false} className={`${input} font-mono`} />
        </Field>
      )}
    </div>
  );
}

type TaskDialogProps = {
  open: boolean;
  onOpenChange: (open: boolean) => void;
  // editing an existing task, or creating one when missing
  task?: ApiScheduledTask;
};

export function TaskDialog({ open, onOpenChange, task }: TaskDialogProps) {
  const { projects } = useWorkspace();
  const queryClient = useQueryClient();
  const initialModel = task?.model ?? DEFAULT_CHAT_SETTINGS.model;
  const [title, setTitle] = useState(task?.title ?? "");
  const [prompt, setPrompt] = useState(task?.prompt ?? "");
  const [model, setModel] = useState(initialModel);
  const [effort, setEffort] = useState(task?.reasoning_effort ?? effortFor(initialModel, DEFAULT_CHAT_SETTINGS.effort));
  const [parts, setParts] = useState<ScheduleParts>(task ? fromCron(task.schedule) : DEFAULT_PARTS);
  const [timezone, setTimezone] = useState(task?.timezone ?? browserTimezone());
  const [project, setProject] = useState(task?.project_id ?? "");
  const cron = toCron(parts);

  const save = useMutation({
    mutationFn: (body: TaskInput) => (task ? trex.scheduled.update(task.id, body) : trex.scheduled.create(body)),
    onSuccess: async () => {
      await queryClient.invalidateQueries({ queryKey: queries.scheduled.all });
      onOpenChange(false);
    },
  });

  function submit(event: FormEvent) {
    event.preventDefault();
    save.mutate({ title, prompt, model, reasoning_effort: effort, schedule: cron, timezone, project_id: project || null });
  }

  return (
    <Dialog.Root open={open} onOpenChange={onOpenChange}>
      <Dialog.Portal>
        <Dialog.Backdrop className={backdrop} />
        <Dialog.Viewport className={dialogViewport}>
          <Dialog.Popup className={`${dialogPopup} max-h-[90vh] max-w-lg overflow-y-auto`}>
            <Dialog.Title className={dialogTitle}>{task ? "Edit scheduled task" : "New scheduled task"}</Dialog.Title>
            <Dialog.Description className={dialogDescription}>Each run starts a new chat. The agent works on its own, without asking you questions.</Dialog.Description>
            <form onSubmit={submit} className="mt-5 space-y-4">
              <Field title="Name">
                <input value={title} onChange={(event) => setTitle(event.target.value)} required maxLength={100} autoFocus placeholder="Morning news digest" className={input} />
              </Field>
              <Field title="Prompt">
                <textarea
                  value={prompt}
                  onChange={(event) => setPrompt(event.target.value)}
                  required
                  rows={4}
                  placeholder="Summarize today's top AI news in five bullet points."
                  className={`ui-input h-auto resize-y py-2.5 leading-6 ${focusRing}`}
                />
              </Field>
              <ScheduleFields parts={parts} onChange={setParts} />
              <Field title="Timezone">
                <select value={timezone} onChange={(event) => setTimezone(event.target.value)} className={input}>
                  {timezones().map((zone) => (
                    <option key={zone} value={zone}>
                      {zone.replaceAll("_", " ")}
                    </option>
                  ))}
                </select>
              </Field>
              <p className="text-xs text-muted">
                {describeSchedule(cron)}, {timezone.replaceAll("_", " ")} time
              </p>
              <div className="grid gap-3 sm:grid-cols-2">
                <Field title="Model">
                  <select
                    value={model}
                    onChange={(event) => {
                      setModel(event.target.value);
                      setEffort(effortFor(event.target.value, effort));
                    }}
                    className={input}
                  >
                    {MODELS.map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </Field>
                <Field title="Thinking">
                  <select value={effort} onChange={(event) => setEffort(event.target.value)} className={input}>
                    {effortsFor(model).map((option) => (
                      <option key={option.value} value={option.value}>
                        {option.label}
                      </option>
                    ))}
                  </select>
                </Field>
              </div>
              {projects.length > 0 && (
                <Field title="Project">
                  <select value={project} onChange={(event) => setProject(event.target.value)} className={input}>
                    <option value="">None</option>
                    {projects.map((option) => (
                      <option key={option.id} value={option.id}>
                        {option.name}
                      </option>
                    ))}
                  </select>
                </Field>
              )}
              {save.error && <p role="alert" className="text-xs text-danger">{save.error.message}</p>}
              <div className="flex justify-end gap-2 pt-2">
                <Dialog.Close render={<Button variant="quiet" />}>Cancel</Dialog.Close>
                <Button type="submit" disabled={save.isPending || !title.trim() || !prompt.trim() || !cron}>
                  {task ? "Save" : "Create"}
                </Button>
              </div>
            </form>
          </Dialog.Popup>
        </Dialog.Viewport>
      </Dialog.Portal>
    </Dialog.Root>
  );
}
