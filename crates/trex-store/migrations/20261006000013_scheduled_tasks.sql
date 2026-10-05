CREATE TABLE scheduled_tasks (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    project_id UUID REFERENCES projects (id) ON DELETE SET NULL,
    title TEXT NOT NULL,
    prompt TEXT NOT NULL,
    model TEXT NOT NULL,
    reasoning_effort TEXT,
    schedule TEXT NOT NULL,
    timezone TEXT NOT NULL,
    paused BOOLEAN NOT NULL DEFAULT FALSE,
    next_run_at TIMESTAMPTZ,
    last_run_at TIMESTAMPTZ,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX scheduled_tasks_workspace ON scheduled_tasks (workspace_id);
CREATE INDEX scheduled_tasks_due ON scheduled_tasks (next_run_at) WHERE NOT paused;

ALTER TABLE sessions ADD COLUMN scheduled_task_id UUID REFERENCES scheduled_tasks (id) ON DELETE SET NULL;
CREATE INDEX sessions_scheduled_task ON sessions (scheduled_task_id) WHERE scheduled_task_id IS NOT NULL;
