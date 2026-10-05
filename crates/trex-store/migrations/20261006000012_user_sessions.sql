ALTER TABLE auth_tokens RENAME TO user_sessions;
ALTER TABLE user_sessions ADD COLUMN ip TEXT, ADD COLUMN last_ip TEXT, ADD COLUMN user_agent TEXT;
CREATE INDEX user_sessions_user ON user_sessions (user_id);
