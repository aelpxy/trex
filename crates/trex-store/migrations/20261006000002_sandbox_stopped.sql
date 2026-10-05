ALTER TABLE sessions ADD COLUMN sandbox_stopped BOOLEAN NOT NULL DEFAULT FALSE;

CREATE INDEX sessions_idle_sandboxes ON sessions (updated_at) WHERE sandbox IS NOT NULL AND NOT sandbox_stopped;
