CREATE TABLE sessions (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    model TEXT NOT NULL,
    reasoning_effort TEXT,
    sandbox TEXT,
    status TEXT NOT NULL DEFAULT 'idle' CHECK (status IN ('idle', 'running', 'needs_input', 'failed')),
    pending_question JSONB,
    last_error TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX sessions_user_created ON sessions (user_id, created_at DESC);

CREATE TABLE session_items (
    session_id UUID NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    seq BIGINT NOT NULL,
    item JSONB NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (session_id, seq)
);

CREATE TABLE usage_records (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL,
    session_id UUID NOT NULL REFERENCES sessions (id) ON DELETE CASCADE,
    model TEXT NOT NULL,
    input_tokens BIGINT NOT NULL,
    cached_input_tokens BIGINT NOT NULL,
    cache_write_tokens BIGINT NOT NULL,
    output_tokens BIGINT NOT NULL,
    reasoning_tokens BIGINT NOT NULL,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX usage_records_user_created ON usage_records (user_id, created_at);
