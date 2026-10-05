ALTER TABLE workspaces ADD COLUMN credits BIGINT NOT NULL DEFAULT 0;

ALTER TABLE workspaces ADD COLUMN credits_refilled_for DATE;

ALTER TABLE usage_records ADD COLUMN credits BIGINT NOT NULL DEFAULT 0;

CREATE TABLE credit_ledger (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    amount BIGINT NOT NULL,
    balance BIGINT NOT NULL,
    kind TEXT NOT NULL CHECK (kind IN ('grant', 'usage', 'adjustment')),
    description TEXT NOT NULL,
    usage_record_id UUID REFERENCES usage_records (id) ON DELETE SET NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX credit_ledger_workspace ON credit_ledger (workspace_id, id DESC);
