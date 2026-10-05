ALTER TABLE usage_records ALTER COLUMN session_id DROP NOT NULL;
ALTER TABLE usage_records DROP CONSTRAINT usage_records_session_id_fkey;
ALTER TABLE usage_records ADD CONSTRAINT usage_records_session_id_fkey FOREIGN KEY (session_id) REFERENCES sessions (id) ON DELETE SET NULL;
