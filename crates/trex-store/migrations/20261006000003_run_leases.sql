ALTER TABLE sessions ADD COLUMN run_id UUID;

ALTER TABLE sessions ADD COLUMN run_heartbeat_at TIMESTAMPTZ;

CREATE INDEX sessions_running ON sessions (run_heartbeat_at) WHERE status = 'running';
