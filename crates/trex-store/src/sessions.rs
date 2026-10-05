use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::Value;
use sqlx::{FromRow, Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

use crate::Store;

// sqlx only accepts static sql, so the shared column list is spliced in at compile time
macro_rules! columns {
    () => {
        "id, workspace_id, project_id, title, model, reasoning_effort, fast, sandbox, sandbox_stopped, status, pending_question, last_error, \
         EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at, EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at"
    };
}

#[derive(Clone, Copy)]
pub enum SessionFilter {
    All,
    NoProject,
    Project(Uuid),
}

#[derive(Clone, Copy, Debug, PartialEq)]
pub enum SessionStatus {
    Idle,
    Running,
    NeedsInput,
    Failed,
}

pub struct Session {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Option<Uuid>,
    pub title: Option<String>,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub fast: bool,
    pub sandbox: Option<String>,
    pub sandbox_stopped: bool,
    pub status: SessionStatus,
    pub pending_question: Option<Value>,
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

// holds the session's row lock, so no run can start until the sandbox is marked stopped
pub struct IdleSandbox {
    pub session: Uuid,
    pub workspace: Uuid,
    pub sandbox: String,
    tx: Transaction<'static, Postgres>,
}

impl IdleSandbox {
    pub async fn mark_stopped(mut self) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET sandbox_stopped = TRUE WHERE id = $1")
            .bind(self.session)
            .execute(&mut *self.tx)
            .await
            .context("failed to mark sandbox stopped")?;
        self.tx
            .commit()
            .await
            .context("failed to mark sandbox stopped")
    }
}

pub struct StaleRun {
    pub session: Uuid,
    pub workspace: Uuid,
    pub run: Uuid,
}

#[derive(Debug, PartialEq)]
pub enum Finish {
    Finished,
    // messages arrived as the run ended; it has to read them before it can finish
    MessagesQueued,
    // another instance took the run over
    NotOwner,
}

pub struct UsageRecord<'a> {
    pub workspace_id: Uuid,
    pub session_id: Uuid,
    pub model: &'a str,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub duration_ms: u64,
}

// one model response's usage, for showing what each turn of a chat cost
pub struct UsageEntry {
    pub created_at_ms: i64,
    pub model: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub credits: i64,
    pub duration_ms: i64,
}

impl SessionStatus {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::Idle => "idle",
            Self::Running => "running",
            Self::NeedsInput => "needs_input",
            Self::Failed => "failed",
        }
    }

    fn parse(status: &str) -> anyhow::Result<Self> {
        Ok(match status {
            "idle" => Self::Idle,
            "running" => Self::Running,
            "needs_input" => Self::NeedsInput,
            "failed" => Self::Failed,
            other => bail!("unknown session status {other}"),
        })
    }
}

impl FromRow<'_, PgRow> for Session {
    fn from_row(row: &PgRow) -> sqlx::Result<Self> {
        let status: String = row.try_get("status")?;
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            project_id: row.try_get("project_id")?,
            title: row.try_get("title")?,
            model: row.try_get("model")?,
            reasoning_effort: row.try_get("reasoning_effort")?,
            fast: row.try_get("fast")?,
            sandbox: row.try_get("sandbox")?,
            sandbox_stopped: row.try_get("sandbox_stopped")?,
            status: SessionStatus::parse(&status)
                .map_err(|error| sqlx::Error::Decode(error.into()))?,
            pending_question: row.try_get("pending_question")?,
            last_error: row.try_get("last_error")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl Store {
    pub async fn create_session(
        &self,
        workspace: Uuid,
        model: &str,
        reasoning_effort: Option<&str>,
        fast: bool,
        project: Option<Uuid>,
    ) -> anyhow::Result<Session> {
        let sql = concat!(
            "INSERT INTO sessions (id, workspace_id, model, reasoning_effort, fast, project_id) ",
            "SELECT $1, $2, $3, $4, $5, $6 ",
            "WHERE $6::UUID IS NULL OR EXISTS (SELECT 1 FROM projects WHERE id = $6 AND workspace_id = $2) RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(Uuid::now_v7())
            .bind(workspace)
            .bind(model)
            .bind(reasoning_effort)
            .bind(fast)
            .bind(project)
            .fetch_optional(&self.pg)
            .await
            .context("failed to create session")?
            .context("the project does not exist")
    }

    // a new chat with the same settings and the first `keep` items of `from`'s history; the
    // sandbox isn't shared, so the branch gets its own when it needs one
    pub async fn branch_session(
        &self,
        workspace: Uuid,
        from: Uuid,
        keep: usize,
    ) -> anyhow::Result<Option<Session>> {
        let mut tx = self
            .pg
            .begin()
            .await
            .context("failed to start a transaction")?;
        let id = Uuid::now_v7();
        let sql = concat!(
            "INSERT INTO sessions (id, workspace_id, project_id, title, model, reasoning_effort, fast, reasoning_model, reasoning_from) ",
            "SELECT $1, workspace_id, project_id, title, model, reasoning_effort, fast, reasoning_model, LEAST(reasoning_from, $4) ",
            "FROM sessions WHERE id = $2 AND workspace_id = $3 RETURNING ",
            columns!()
        );
        let Some(session): Option<Session> = sqlx::query_as(sql)
            .bind(id)
            .bind(from)
            .bind(workspace)
            .bind(i32::try_from(keep).context("history is too long to branch")?)
            .fetch_optional(&mut *tx)
            .await
            .context("failed to create the branch")?
        else {
            return Ok(None);
        };
        sqlx::query(
            "INSERT INTO session_items (session_id, seq, item, created_at) \
             SELECT $1, seq, item, created_at FROM session_items WHERE session_id = $2 AND seq <= $3",
        )
        .bind(id)
        .bind(from)
        .bind(i64::try_from(keep).context("history is too long to branch")?)
        .execute(&mut *tx)
        .await
        .context("failed to copy the history into the branch")?;
        tx.commit().await.context("failed to commit the branch")?;
        Ok(Some(session))
    }

    pub async fn session(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<Option<Session>> {
        let sql = concat!(
            "SELECT ",
            columns!(),
            " FROM sessions WHERE id = $1 AND workspace_id = $2"
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(workspace)
            .fetch_optional(&self.pg)
            .await
            .context("failed to load session")
    }

    // uuid v7 ids sort by creation time, so they double as the pagination cursor
    pub async fn sessions(
        &self,
        workspace: Uuid,
        limit: i64,
        before: Option<Uuid>,
        filter: SessionFilter,
    ) -> anyhow::Result<Vec<Session>> {
        let sql = concat!(
            "SELECT ",
            columns!(),
            " FROM sessions WHERE workspace_id = $1 AND ($2::UUID IS NULL OR id < $2) ",
            "AND ($4 = 'all' OR ($4 = 'none' AND project_id IS NULL) OR project_id = $5) ORDER BY id DESC LIMIT $3"
        );
        let (kind, project) = match filter {
            SessionFilter::All => ("all", None),
            SessionFilter::NoProject => ("none", None),
            SessionFilter::Project(project) => ("project", Some(project)),
        };
        sqlx::query_as(sql)
            .bind(workspace)
            .bind(before)
            .bind(limit)
            .bind(kind)
            .bind(project)
            .fetch_all(&self.pg)
            .await
            .context("failed to list sessions")
    }

    // `project` of Some(None) moves the session out of its project; none if either doesn't exist
    pub async fn update_session(
        &self,
        workspace: Uuid,
        id: Uuid,
        title: Option<&str>,
        project: Option<Option<Uuid>>,
    ) -> anyhow::Result<Option<Session>> {
        let sql = concat!(
            "UPDATE sessions SET title = COALESCE($3, title), ",
            "project_id = CASE WHEN $4 THEN $5 ELSE project_id END, updated_at = NOW() ",
            "WHERE id = $1 AND workspace_id = $2 ",
            "AND ($5::UUID IS NULL OR EXISTS (SELECT 1 FROM projects WHERE id = $5 AND workspace_id = $2)) RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(workspace)
            .bind(title)
            .bind(project.is_some())
            .bind(project.flatten())
            .fetch_optional(&self.pg)
            .await
            .context("failed to update session")
    }

    pub async fn set_session_model(
        &self,
        workspace: Uuid,
        id: Uuid,
        model: &str,
        reasoning_effort: Option<&str>,
        fast: bool,
    ) -> anyhow::Result<Option<Session>> {
        let sql = concat!(
            "UPDATE sessions SET model = $3, reasoning_effort = $4, fast = $5, updated_at = NOW() ",
            "WHERE id = $1 AND workspace_id = $2 RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(workspace)
            .bind(model)
            .bind(reasoning_effort)
            .bind(fast)
            .fetch_optional(&self.pg)
            .await
            .context("failed to update session model")
    }

    // reasoning is only readable by the model that wrote it, so a run on a different model than the
    // last one ignores the reasoning saved before `items`; returns where readable reasoning starts
    pub async fn start_reasoning(
        &self,
        workspace: Uuid,
        id: Uuid,
        model: &str,
        items: i32,
    ) -> anyhow::Result<usize> {
        let from: i32 = sqlx::query_scalar(concat!(
            "UPDATE sessions SET reasoning_from = CASE WHEN reasoning_model IS DISTINCT FROM $3 ",
            "AND reasoning_model IS NOT NULL THEN $4 ELSE reasoning_from END, reasoning_model = $3 ",
            "WHERE id = $1 AND workspace_id = $2 RETURNING reasoning_from"
        ))
        .bind(id)
        .bind(workspace)
        .bind(model)
        .bind(items)
        .fetch_one(&self.pg)
        .await
        .context("failed to record the session's reasoning model")?;
        Ok(usize::try_from(from).unwrap_or(0))
    }

    // a generated title never replaces one the user set meanwhile
    pub async fn set_title_if_missing(
        &self,
        workspace: Uuid,
        id: Uuid,
        title: &str,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET title = $3 WHERE id = $1 AND workspace_id = $2 AND title IS NULL",
        )
        .bind(id)
        .bind(workspace)
        .bind(title)
        .execute(&self.pg)
        .await
        .context("failed to set session title")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn delete_session(
        &self,
        workspace: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<Session>> {
        let sql = concat!(
            "DELETE FROM sessions WHERE id = $1 AND workspace_id = $2 RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(workspace)
            .fetch_optional(&self.pg)
            .await
            .context("failed to delete session")
    }

    pub async fn set_session_sandbox(
        &self,
        workspace: Uuid,
        id: Uuid,
        sandbox: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE sessions SET sandbox = $3, sandbox_stopped = FALSE, updated_at = NOW() WHERE id = $1 AND workspace_id = $2",
        )
        .bind(id)
        .bind(workspace)
        .bind(sandbox)
        .execute(&self.pg)
        .await
        .context("failed to save session sandbox")?;
        Ok(())
    }

    // the oldest session whose sandbox has idled for `idle`; rows other instances hold are skipped
    pub async fn next_idle_sandbox(&self, idle: Duration) -> anyhow::Result<Option<IdleSandbox>> {
        let mut tx = self
            .pg
            .begin()
            .await
            .context("failed to find idle sandboxes")?;
        let row = sqlx::query(
            "SELECT id, workspace_id, sandbox FROM sessions \
             WHERE sandbox IS NOT NULL AND NOT sandbox_stopped AND status <> 'running' \
             AND updated_at < NOW() - MAKE_INTERVAL(secs => $1) \
             ORDER BY updated_at LIMIT 1 FOR UPDATE SKIP LOCKED",
        )
        .bind(idle.as_secs_f64())
        .fetch_optional(&mut *tx)
        .await
        .context("failed to find idle sandboxes")?;
        let Some(row) = row else {
            return Ok(None);
        };
        Ok(Some(IdleSandbox {
            session: row.try_get("id")?,
            workspace: row.try_get("workspace_id")?,
            sandbox: row.try_get("sandbox")?,
            tx,
        }))
    }

    // using a session's sandbox, e.g. browsing its files, keeps it from being stopped as idle
    pub async fn touch_session(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET updated_at = NOW() WHERE id = $1 AND workspace_id = $2")
            .bind(id)
            .bind(workspace)
            .execute(&self.pg)
            .await
            .context("failed to touch session")?;
        Ok(())
    }

    pub async fn mark_sandbox_started(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE sessions SET sandbox_stopped = FALSE WHERE id = $1 AND workspace_id = $2",
        )
        .bind(id)
        .bind(workspace)
        .execute(&self.pg)
        .await
        .context("failed to mark sandbox started")?;
        Ok(())
    }

    // the conditional update is what guarantees one run per session, even across trex instances;
    // `run` owns the session until it finishes or its heartbeat goes stale
    pub async fn start_run(&self, workspace: Uuid, id: Uuid, run: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET status = 'running', pending_question = NULL, last_error = NULL, \
             run_id = $3, run_heartbeat_at = NOW(), updated_at = NOW() \
             WHERE id = $1 AND workspace_id = $2 AND status <> 'running'",
        )
        .bind(id)
        .bind(workspace)
        .bind(run)
        .execute(&self.pg)
        .await
        .context("failed to start run")?;
        Ok(result.rows_affected() > 0)
    }

    // false once another instance has taken the run over
    pub async fn heartbeat_run(&self, id: Uuid, run: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET run_heartbeat_at = NOW() WHERE id = $1 AND run_id = $2 AND status = 'running'",
        )
        .bind(id)
        .bind(run)
        .execute(&self.pg)
        .await
        .context("failed to renew run lease")?;
        Ok(result.rows_affected() > 0)
    }

    // takes over runs whose instance stopped renewing them, e.g. after a restart or a crash
    pub async fn claim_stale_runs(
        &self,
        stale_after: Duration,
        limit: i64,
    ) -> anyhow::Result<Vec<StaleRun>> {
        let rows = sqlx::query(
            "UPDATE sessions SET run_id = GEN_RANDOM_UUID(), run_heartbeat_at = NOW() \
             WHERE id IN (SELECT id FROM sessions WHERE status = 'running' \
             AND (run_heartbeat_at IS NULL OR run_heartbeat_at < NOW() - MAKE_INTERVAL(secs => $1)) \
             ORDER BY run_heartbeat_at NULLS FIRST LIMIT $2 FOR UPDATE SKIP LOCKED) \
             RETURNING id, workspace_id, run_id",
        )
        .bind(stale_after.as_secs_f64())
        .bind(limit)
        .fetch_all(&self.pg)
        .await
        .context("failed to claim stale runs")?;
        rows.iter()
            .map(|row| {
                Ok(StaleRun {
                    session: row.try_get("id")?,
                    workspace: row.try_get("workspace_id")?,
                    run: row.try_get("run_id")?,
                })
            })
            .collect()
    }

    pub async fn finish_run(
        &self,
        workspace: Uuid,
        id: Uuid,
        run: Uuid,
        status: SessionStatus,
        pending_question: Option<&Value>,
        last_error: Option<&str>,
    ) -> anyhow::Result<Finish> {
        let result = sqlx::query(
            "UPDATE sessions SET status = $4, pending_question = $5, last_error = $6, run_id = NULL, \
             run_heartbeat_at = NULL, updated_at = NOW() \
             WHERE id = $1 AND workspace_id = $2 AND run_id = $3 AND status = 'running' \
             AND queued_messages = '[]'::JSONB",
        )
        .bind(id)
        .bind(workspace)
        .bind(run)
        .bind(status.as_str())
        .bind(pending_question)
        .bind(last_error)
        .execute(&self.pg)
        .await
        .context("failed to finish run")?;
        if result.rows_affected() > 0 {
            return Ok(Finish::Finished);
        }
        let owned: Option<bool> = sqlx::query_scalar(
            "SELECT run_id = $3 AND status = 'running' FROM sessions WHERE id = $1 AND workspace_id = $2",
        )
        .bind(id)
        .bind(workspace)
        .bind(run)
        .fetch_optional(&self.pg)
        .await
        .context("failed to finish run")?
        .flatten();
        Ok(if owned == Some(true) {
            Finish::MessagesQueued
        } else {
            Finish::NotOwner
        })
    }

    // only a running session takes messages into its queue; otherwise the caller starts a run
    pub async fn queue_message(
        &self,
        workspace: Uuid,
        id: Uuid,
        item: &Value,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET queued_messages = queued_messages || JSONB_BUILD_ARRAY($3::JSONB), updated_at = NOW() \
             WHERE id = $1 AND workspace_id = $2 AND status = 'running'",
        )
        .bind(id)
        .bind(workspace)
        .bind(item)
        .execute(&self.pg)
        .await
        .context("failed to queue message")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn take_queued_messages(
        &self,
        workspace: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Vec<Value>> {
        let queued: Option<Value> = sqlx::query_scalar(
            "UPDATE sessions s SET queued_messages = '[]'::JSONB \
             FROM (SELECT id, queued_messages FROM sessions WHERE id = $1 AND workspace_id = $2 FOR UPDATE) old \
             WHERE s.id = old.id RETURNING old.queued_messages",
        )
        .bind(id)
        .bind(workspace)
        .fetch_optional(&self.pg)
        .await
        .context("failed to take queued messages")?;
        queued
            .map(serde_json::from_value)
            .transpose()
            .context("invalid queued messages")
            .map(Option::unwrap_or_default)
    }

    pub async fn session_items(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<Vec<Value>> {
        let rows = sqlx::query(
            "SELECT i.item FROM session_items i JOIN sessions s ON s.id = i.session_id \
             WHERE i.session_id = $1 AND s.workspace_id = $2 ORDER BY i.seq",
        )
        .bind(id)
        .bind(workspace)
        .fetch_all(&self.pg)
        .await
        .context("failed to load session items")?;
        rows.iter()
            .map(|row| row.try_get("item").context("invalid session item"))
            .collect()
    }

    // with when each item was saved, in unix milliseconds
    pub async fn session_items_timed(
        &self,
        workspace: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Vec<(Value, i64, i64)>> {
        let rows = sqlx::query(
            "SELECT i.item, i.seq, (EXTRACT(EPOCH FROM i.created_at) * 1000)::BIGINT AS created_at_ms \
             FROM session_items i JOIN sessions s ON s.id = i.session_id \
             WHERE i.session_id = $1 AND s.workspace_id = $2 ORDER BY i.seq",
        )
        .bind(id)
        .bind(workspace)
        .fetch_all(&self.pg)
        .await
        .context("failed to load session items")?;
        rows.iter()
            .map(|row| {
                Ok((
                    row.try_get("item")?,
                    row.try_get("seq")?,
                    row.try_get("created_at_ms")?,
                ))
            })
            .collect()
    }

    pub async fn count_session_items(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<i64> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM session_items i JOIN sessions s ON s.id = i.session_id \
             WHERE i.session_id = $1 AND s.workspace_id = $2",
        )
        .bind(id)
        .bind(workspace)
        .fetch_one(&self.pg)
        .await
        .context("failed to count session items")
    }

    pub async fn session_usage(
        &self,
        workspace: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Vec<UsageEntry>> {
        let rows = sqlx::query(
            "SELECT (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT AS created_at_ms, model, input_tokens, \
             cached_input_tokens, output_tokens, reasoning_tokens, credits, duration_ms \
             FROM usage_records WHERE session_id = $1 AND workspace_id = $2 ORDER BY created_at",
        )
        .bind(id)
        .bind(workspace)
        .fetch_all(&self.pg)
        .await
        .context("failed to load session usage")?;
        rows.iter()
            .map(|row| {
                Ok(UsageEntry {
                    created_at_ms: row.try_get("created_at_ms")?,
                    model: row.try_get("model")?,
                    input_tokens: row.try_get("input_tokens")?,
                    cached_input_tokens: row.try_get("cached_input_tokens")?,
                    output_tokens: row.try_get("output_tokens")?,
                    reasoning_tokens: row.try_get("reasoning_tokens")?,
                    credits: row.try_get("credits")?,
                    duration_ms: row.try_get("duration_ms")?,
                })
            })
            .collect()
    }

    pub async fn append_session_items(
        &self,
        workspace: Uuid,
        id: Uuid,
        items: &[Value],
    ) -> anyhow::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO session_items (session_id, seq, item) \
             SELECT s.id, (SELECT COALESCE(MAX(seq), 0) FROM session_items WHERE session_id = s.id) + t.ord, t.item \
             FROM sessions s, UNNEST($3::JSONB[]) WITH ORDINALITY AS t (item, ord) \
             WHERE s.id = $1 AND s.workspace_id = $2",
        )
        .bind(id)
        .bind(workspace)
        .bind(items)
        .execute(&self.pg)
        .await
        .context("failed to save session items")?;
        Ok(())
    }

    // items are keyed by their position in history, so saving the same items again is a no-op
    pub async fn put_session_items(
        &self,
        workspace: Uuid,
        id: Uuid,
        first: usize,
        items: &[Value],
    ) -> anyhow::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO session_items (session_id, seq, item) \
             SELECT s.id, $3 + t.ord, t.item \
             FROM sessions s, UNNEST($4::JSONB[]) WITH ORDINALITY AS t (item, ord) \
             WHERE s.id = $1 AND s.workspace_id = $2 \
             ON CONFLICT (session_id, seq) DO NOTHING",
        )
        .bind(id)
        .bind(workspace)
        .bind(first as i64)
        .bind(items)
        .execute(&self.pg)
        .await
        .context("failed to save session items")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use std::time::Duration;

    use serde_json::json;

    use super::*;

    // needs TREX_DATABASE_URL and TREX_REDIS_URL: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn session_lifecycle() {
        dotenvy::dotenv().ok();
        let store = Store::connect(
            &std::env::var("TREX_DATABASE_URL").unwrap(),
            &std::env::var("TREX_REDIS_URL").unwrap(),
        )
        .await
        .unwrap();
        let workspace = |name: &'static str| {
            let store = &store;
            async move {
                let email = format!("{name}-{}@test.trex", Uuid::now_v7());
                let (_, workspace) = store
                    .create_user(&email, name, "hash")
                    .await
                    .unwrap()
                    .unwrap();
                workspace.id
            }
        };
        let (alice, mallory) = (workspace("alice").await, workspace("mallory").await);

        let project = store.create_project(alice, "Backend", None).await.unwrap();
        assert!(
            store
                .create_session(mallory, "gpt-6.1-sol", None, false, Some(project.id))
                .await
                .is_err(),
            "no sessions in another workspace's project"
        );
        let in_project = store
            .create_session(alice, "gpt-6.1-sol", None, false, Some(project.id))
            .await
            .unwrap();

        let session = store
            .create_session(alice, "gpt-6.1-sol", Some("low"), true, None)
            .await
            .unwrap();
        assert_eq!(session.status, SessionStatus::Idle);
        assert!(session.fast);
        assert!(store.session(mallory, session.id).await.unwrap().is_none());
        assert!(
            store
                .delete_session(mallory, session.id)
                .await
                .unwrap()
                .is_none()
        );
        let ids = |sessions: Vec<Session>| sessions.into_iter().map(|s| s.id).collect::<Vec<_>>();
        assert_eq!(
            ids(store
                .sessions(alice, 10, None, SessionFilter::All)
                .await
                .unwrap()),
            [session.id, in_project.id]
        );
        assert_eq!(
            ids(store
                .sessions(alice, 10, None, SessionFilter::NoProject)
                .await
                .unwrap()),
            [session.id]
        );
        assert_eq!(
            ids(store
                .sessions(alice, 10, None, SessionFilter::Project(project.id))
                .await
                .unwrap()),
            [in_project.id]
        );

        assert!(
            store
                .set_title_if_missing(alice, session.id, "Generated")
                .await
                .unwrap()
        );
        assert!(
            !store
                .set_title_if_missing(alice, session.id, "Again")
                .await
                .unwrap()
        );
        let renamed = store
            .update_session(alice, session.id, Some("Renamed"), Some(Some(project.id)))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (renamed.title.as_deref(), renamed.project_id),
            (Some("Renamed"), Some(project.id))
        );
        let other = store.create_project(mallory, "Theirs", None).await.unwrap();
        assert!(
            store
                .update_session(alice, session.id, None, Some(Some(other.id)))
                .await
                .unwrap()
                .is_none(),
            "can't move a chat into another workspace's project"
        );
        assert!(
            store
                .update_session(mallory, session.id, Some("x"), None)
                .await
                .unwrap()
                .is_none()
        );
        store.delete_project(alice, project.id).await.unwrap();
        let kept = store.session(alice, in_project.id).await.unwrap().unwrap();
        assert_eq!(kept.project_id, None, "deleting a project keeps its chats");
        let moved_back = store.session(alice, session.id).await.unwrap().unwrap();
        assert_eq!(
            (moved_back.title.as_deref(), moved_back.project_id),
            (Some("Renamed"), None)
        );
        assert!(
            store
                .sessions(alice, 10, Some(in_project.id), SessionFilter::All)
                .await
                .unwrap()
                .is_empty()
        );

        let (run, other) = (Uuid::now_v7(), Uuid::now_v7());
        assert!(store.start_run(alice, session.id, run).await.unwrap());
        assert!(
            !store.start_run(alice, session.id, other).await.unwrap(),
            "one run at a time"
        );
        assert!(!store.start_run(mallory, session.id, other).await.unwrap());
        assert!(store.heartbeat_run(session.id, run).await.unwrap());
        assert!(!store.heartbeat_run(session.id, other).await.unwrap());

        let item = |n: i32| json!({ "n": n });
        store
            .put_session_items(alice, session.id, 0, &[item(1), item(2)])
            .await
            .unwrap();
        store
            .put_session_items(alice, session.id, 1, &[item(2), item(3)])
            .await
            .unwrap();
        store
            .put_session_items(mallory, session.id, 3, &[item(666)])
            .await
            .unwrap();
        store
            .append_session_items(alice, session.id, &[item(4)])
            .await
            .unwrap();
        let items = store.session_items(alice, session.id).await.unwrap();
        assert_eq!(
            items,
            [item(1), item(2), item(3), item(4)],
            "saving again is a no-op"
        );
        assert!(
            store
                .session_items(mallory, session.id)
                .await
                .unwrap()
                .is_empty()
        );

        assert!(
            store
                .queue_message(alice, session.id, &json!("first"))
                .await
                .unwrap()
        );
        assert!(
            store
                .queue_message(alice, session.id, &json!({"n": 2}))
                .await
                .unwrap()
        );
        assert!(
            !store
                .queue_message(mallory, session.id, &json!("evil"))
                .await
                .unwrap()
        );
        assert_eq!(
            store
                .finish_run(alice, session.id, run, SessionStatus::Idle, None, None)
                .await
                .unwrap(),
            Finish::MessagesQueued
        );
        assert!(
            store
                .take_queued_messages(mallory, session.id)
                .await
                .unwrap()
                .is_empty()
        );
        assert_eq!(
            store.take_queued_messages(alice, session.id).await.unwrap(),
            [json!("first"), json!({"n": 2})]
        );
        assert!(
            store
                .take_queued_messages(alice, session.id)
                .await
                .unwrap()
                .is_empty()
        );

        sqlx::query(
            "UPDATE sessions SET run_heartbeat_at = NOW() - INTERVAL '1 hour' WHERE id = $1",
        )
        .bind(session.id)
        .execute(&store.pg)
        .await
        .unwrap();
        let stale = store
            .claim_stale_runs(Duration::from_secs(30), 1000)
            .await
            .unwrap();
        let resumed = stale
            .iter()
            .find(|stale| stale.session == session.id)
            .expect("the stale run is claimed");
        assert_eq!(resumed.workspace, alice);
        assert_ne!(resumed.run, run);
        assert!(
            !store.heartbeat_run(session.id, run).await.unwrap(),
            "the old run lost its lease"
        );
        assert_eq!(
            store
                .finish_run(alice, session.id, run, SessionStatus::Idle, None, None)
                .await
                .unwrap(),
            Finish::NotOwner
        );
        assert!(
            store
                .claim_stale_runs(Duration::from_secs(30), 1000)
                .await
                .unwrap()
                .iter()
                .all(|stale| stale.session != session.id),
            "a fresh lease isn't stale"
        );

        let question = json!({"call_id": "call_1"});
        assert_eq!(
            store
                .finish_run(
                    alice,
                    session.id,
                    resumed.run,
                    SessionStatus::NeedsInput,
                    Some(&question),
                    None,
                )
                .await
                .unwrap(),
            Finish::Finished
        );
        assert!(
            !store
                .queue_message(alice, session.id, &json!("late"))
                .await
                .unwrap(),
            "only running sessions queue"
        );
        let reloaded = store.session(alice, session.id).await.unwrap().unwrap();
        assert_eq!(reloaded.status, SessionStatus::NeedsInput);
        assert_eq!(reloaded.pending_question, Some(question));

        store
            .charge_usage(
                &UsageRecord {
                    workspace_id: alice,
                    session_id: session.id,
                    model: "gpt-6.1-sol",
                    input_tokens: 10,
                    cached_input_tokens: 2,
                    cache_write_tokens: 0,
                    output_tokens: 5,
                    reasoning_tokens: 1,
                    duration_ms: 1500,
                },
                7,
            )
            .await
            .unwrap();
        let usage = store.session_usage(alice, session.id).await.unwrap();
        assert_eq!(
            usage
                .iter()
                .map(|entry| (entry.input_tokens, entry.credits, entry.duration_ms))
                .collect::<Vec<_>>(),
            [(10, 7, 1500)]
        );
        assert!(
            store
                .session_usage(mallory, session.id)
                .await
                .unwrap()
                .is_empty()
        );
        let timed = store.session_items_timed(alice, session.id).await.unwrap();
        assert!(!timed.is_empty() && timed.iter().all(|(_, _, at)| *at > 1_700_000_000_000));
        assert_eq!(
            store.count_session_items(alice, session.id).await.unwrap(),
            timed.len() as i64
        );

        let first = store
            .publish_event(session.id, &json!({"type": "a"}))
            .await
            .unwrap();
        store
            .publish_event(session.id, &json!({"type": "b"}))
            .await
            .unwrap();
        let mut connection = store.event_connection().await.unwrap();
        let all = store
            .read_events(&mut connection, session.id, "0", Duration::from_millis(100))
            .await
            .unwrap();
        let resumed = store
            .read_events(
                &mut connection,
                session.id,
                &first,
                Duration::from_millis(100),
            )
            .await
            .unwrap();
        let none = store
            .read_events(&mut connection, session.id, "$", Duration::from_secs(2))
            .await
            .unwrap();
        assert_eq!(all.len(), 2);
        assert_eq!(resumed.len(), 1);
        assert_eq!(resumed[0].event, json!({"type": "b"}));
        assert!(none.is_empty());
        assert_eq!(
            store.last_event_id(session.id).await.unwrap().as_deref(),
            Some(resumed[0].id.as_str())
        );

        store
            .delete_session(alice, session.id)
            .await
            .unwrap()
            .unwrap();
        store.delete_events(session.id).await.unwrap();
        let usage_left: i64 =
            sqlx::query_scalar("SELECT COUNT(*) FROM usage_records WHERE session_id = $1")
                .bind(session.id)
                .fetch_one(&store.pg)
                .await
                .unwrap();
        assert_eq!(usage_left, 0);
        assert!(store.last_event_id(session.id).await.unwrap().is_none());
        sqlx::query("DELETE FROM users WHERE id IN (SELECT user_id FROM workspace_members WHERE workspace_id = ANY($1))")
            .bind([alice, mallory])
            .execute(&store.pg)
            .await
            .unwrap();
        sqlx::query("DELETE FROM workspaces WHERE id = ANY($1)")
            .bind([alice, mallory])
            .execute(&store.pg)
            .await
            .unwrap();
    }
}
