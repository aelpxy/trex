CREATE TABLE users (
    id UUID PRIMARY KEY,
    email TEXT NOT NULL,
    name TEXT NOT NULL,
    password_hash TEXT NOT NULL,
    email_verified_at TIMESTAMPTZ,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE UNIQUE INDEX users_email ON users (LOWER(email));

CREATE TABLE workspaces (
    id UUID PRIMARY KEY,
    name TEXT NOT NULL,
    plan TEXT NOT NULL DEFAULT 'free',
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE TABLE workspace_members (
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    role TEXT NOT NULL CHECK (role IN ('owner', 'member')),
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    PRIMARY KEY (workspace_id, user_id)
);

CREATE INDEX workspace_members_user ON workspace_members (user_id);

CREATE TABLE auth_tokens (
    id UUID PRIMARY KEY,
    user_id UUID NOT NULL REFERENCES users (id) ON DELETE CASCADE,
    token_hash BYTEA NOT NULL UNIQUE,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    last_used_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    expires_at TIMESTAMPTZ NOT NULL
);

CREATE INDEX auth_tokens_user ON auth_tokens (user_id);

INSERT INTO workspaces (id, name)
SELECT user_id, 'Personal' FROM sessions
UNION
SELECT user_id, 'Personal' FROM usage_records;

ALTER TABLE sessions RENAME COLUMN user_id TO workspace_id;

ALTER TABLE sessions ADD CONSTRAINT sessions_workspace FOREIGN KEY (workspace_id) REFERENCES workspaces (id) ON DELETE CASCADE;

ALTER INDEX sessions_user_created RENAME TO sessions_workspace_created;

ALTER TABLE usage_records RENAME COLUMN user_id TO workspace_id;

ALTER INDEX usage_records_user_created RENAME TO usage_records_workspace_created;

CREATE TABLE projects (
    id UUID PRIMARY KEY,
    workspace_id UUID NOT NULL REFERENCES workspaces (id) ON DELETE CASCADE,
    name TEXT NOT NULL,
    instructions TEXT,
    created_at TIMESTAMPTZ NOT NULL DEFAULT NOW(),
    updated_at TIMESTAMPTZ NOT NULL DEFAULT NOW()
);

CREATE INDEX projects_workspace ON projects (workspace_id, id DESC);

ALTER TABLE sessions ADD COLUMN project_id UUID REFERENCES projects (id) ON DELETE SET NULL;

ALTER TABLE sessions ADD COLUMN title TEXT;

CREATE INDEX sessions_project ON sessions (project_id, id DESC) WHERE project_id IS NOT NULL;
