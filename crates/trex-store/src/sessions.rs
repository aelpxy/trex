use std::time::Duration;

use anyhow::{Context, bail};
use serde_json::Value;
use sqlx::{FromRow, Postgres, Row, Transaction, postgres::PgRow};
use uuid::Uuid;

use crate::Store;

// sqlx only accepts static sql, so the shared column list is spliced in at compile time
macro_rules! columns {
    () => {
        "id, user_id, model, reasoning_effort, fast, sandbox, sandbox_stopped, status, pending_question, last_error, \
         EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at, EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at"
    };
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
    pub user_id: Uuid,
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
    pub user: Uuid,
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
    pub user: Uuid,
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
    pub user_id: Uuid,
    pub session_id: Uuid,
    pub model: &'a str,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
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
            user_id: row.try_get("user_id")?,
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
        user: Uuid,
        model: &str,
        reasoning_effort: Option<&str>,
        fast: bool,
    ) -> anyhow::Result<Session> {
        let sql = concat!(
            "INSERT INTO sessions (id, user_id, model, reasoning_effort, fast) VALUES ($1, $2, $3, $4, $5) RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(Uuid::now_v7())
            .bind(user)
            .bind(model)
            .bind(reasoning_effort)
            .bind(fast)
            .fetch_one(&self.pg)
            .await
            .context("failed to create session")
    }

    pub async fn session(&self, user: Uuid, id: Uuid) -> anyhow::Result<Option<Session>> {
        let sql = concat!(
            "SELECT ",
            columns!(),
            " FROM sessions WHERE id = $1 AND user_id = $2"
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(user)
            .fetch_optional(&self.pg)
            .await
            .context("failed to load session")
    }

    // uuid v7 ids sort by creation time, so they double as the pagination cursor
    pub async fn sessions(
        &self,
        user: Uuid,
        limit: i64,
        before: Option<Uuid>,
    ) -> anyhow::Result<Vec<Session>> {
        let sql = concat!(
            "SELECT ",
            columns!(),
            " FROM sessions WHERE user_id = $1 AND ($2::UUID IS NULL OR id < $2) ORDER BY id DESC LIMIT $3"
        );
        sqlx::query_as(sql)
            .bind(user)
            .bind(before)
            .bind(limit)
            .fetch_all(&self.pg)
            .await
            .context("failed to list sessions")
    }

    pub async fn delete_session(&self, user: Uuid, id: Uuid) -> anyhow::Result<Option<Session>> {
        let sql = concat!(
            "DELETE FROM sessions WHERE id = $1 AND user_id = $2 RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(user)
            .fetch_optional(&self.pg)
            .await
            .context("failed to delete session")
    }

    pub async fn set_session_sandbox(
        &self,
        user: Uuid,
        id: Uuid,
        sandbox: &str,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE sessions SET sandbox = $3, updated_at = NOW() WHERE id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(user)
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
            "SELECT id, user_id, sandbox FROM sessions \
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
            user: row.try_get("user_id")?,
            sandbox: row.try_get("sandbox")?,
            tx,
        }))
    }

    pub async fn mark_sandbox_started(&self, user: Uuid, id: Uuid) -> anyhow::Result<()> {
        sqlx::query("UPDATE sessions SET sandbox_stopped = FALSE WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user)
            .execute(&self.pg)
            .await
            .context("failed to mark sandbox started")?;
        Ok(())
    }

    // the conditional update is what guarantees one run per session, even across trex instances;
    // `run` owns the session until it finishes or its heartbeat goes stale
    pub async fn start_run(&self, user: Uuid, id: Uuid, run: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET status = 'running', pending_question = NULL, last_error = NULL, \
             run_id = $3, run_heartbeat_at = NOW(), updated_at = NOW() \
             WHERE id = $1 AND user_id = $2 AND status <> 'running'",
        )
        .bind(id)
        .bind(user)
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
             RETURNING id, user_id, run_id",
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
                    user: row.try_get("user_id")?,
                    run: row.try_get("run_id")?,
                })
            })
            .collect()
    }

    pub async fn finish_run(
        &self,
        user: Uuid,
        id: Uuid,
        run: Uuid,
        status: SessionStatus,
        pending_question: Option<&Value>,
        last_error: Option<&str>,
    ) -> anyhow::Result<Finish> {
        let result = sqlx::query(
            "UPDATE sessions SET status = $4, pending_question = $5, last_error = $6, run_id = NULL, \
             run_heartbeat_at = NULL, updated_at = NOW() \
             WHERE id = $1 AND user_id = $2 AND run_id = $3 AND status = 'running' \
             AND queued_messages = '[]'::JSONB",
        )
        .bind(id)
        .bind(user)
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
            "SELECT run_id = $3 AND status = 'running' FROM sessions WHERE id = $1 AND user_id = $2",
        )
        .bind(id)
        .bind(user)
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
    pub async fn queue_message(&self, user: Uuid, id: Uuid, item: &Value) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET queued_messages = queued_messages || JSONB_BUILD_ARRAY($3::JSONB), updated_at = NOW() \
             WHERE id = $1 AND user_id = $2 AND status = 'running'",
        )
        .bind(id)
        .bind(user)
        .bind(item)
        .execute(&self.pg)
        .await
        .context("failed to queue message")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn take_queued_messages(&self, user: Uuid, id: Uuid) -> anyhow::Result<Vec<Value>> {
        let queued: Option<Value> = sqlx::query_scalar(
            "UPDATE sessions s SET queued_messages = '[]'::JSONB \
             FROM (SELECT id, queued_messages FROM sessions WHERE id = $1 AND user_id = $2 FOR UPDATE) old \
             WHERE s.id = old.id RETURNING old.queued_messages",
        )
        .bind(id)
        .bind(user)
        .fetch_optional(&self.pg)
        .await
        .context("failed to take queued messages")?;
        queued
            .map(serde_json::from_value)
            .transpose()
            .context("invalid queued messages")
            .map(Option::unwrap_or_default)
    }

    pub async fn session_items(&self, user: Uuid, id: Uuid) -> anyhow::Result<Vec<Value>> {
        let rows = sqlx::query(
            "SELECT i.item FROM session_items i JOIN sessions s ON s.id = i.session_id \
             WHERE i.session_id = $1 AND s.user_id = $2 ORDER BY i.seq",
        )
        .bind(id)
        .bind(user)
        .fetch_all(&self.pg)
        .await
        .context("failed to load session items")?;
        rows.iter()
            .map(|row| row.try_get("item").context("invalid session item"))
            .collect()
    }

    pub async fn append_session_items(
        &self,
        user: Uuid,
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
             WHERE s.id = $1 AND s.user_id = $2",
        )
        .bind(id)
        .bind(user)
        .bind(items)
        .execute(&self.pg)
        .await
        .context("failed to save session items")?;
        Ok(())
    }

    // items are keyed by their position in history, so saving the same items again is a no-op
    pub async fn put_session_items(
        &self,
        user: Uuid,
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
             WHERE s.id = $1 AND s.user_id = $2 \
             ON CONFLICT (session_id, seq) DO NOTHING",
        )
        .bind(id)
        .bind(user)
        .bind(first as i64)
        .bind(items)
        .execute(&self.pg)
        .await
        .context("failed to save session items")?;
        Ok(())
    }

    pub async fn record_usage(&self, usage: &UsageRecord<'_>) -> anyhow::Result<()> {
        sqlx::query(
            "INSERT INTO usage_records (id, user_id, session_id, model, input_tokens, cached_input_tokens, \
             cache_write_tokens, output_tokens, reasoning_tokens) VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9)",
        )
        .bind(Uuid::now_v7())
        .bind(usage.user_id)
        .bind(usage.session_id)
        .bind(usage.model)
        .bind(usage.input_tokens as i64)
        .bind(usage.cached_input_tokens as i64)
        .bind(usage.cache_write_tokens as i64)
        .bind(usage.output_tokens as i64)
        .bind(usage.reasoning_tokens as i64)
        .execute(&self.pg)
        .await
        .context("failed to record usage")?;
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
        let (alice, mallory) = (Uuid::now_v7(), Uuid::now_v7());

        let session = store
            .create_session(alice, "gpt-6.1-sol", Some("low"), true)
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
        assert_eq!(store.sessions(alice, 10, None).await.unwrap().len(), 1);
        assert!(
            store
                .sessions(alice, 10, Some(session.id))
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
        assert_eq!(resumed.user, alice);
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
            .record_usage(&UsageRecord {
                user_id: alice,
                session_id: session.id,
                model: "gpt-6.1-sol",
                input_tokens: 10,
                cached_input_tokens: 2,
                cache_write_tokens: 0,
                output_tokens: 5,
                reasoning_tokens: 1,
            })
            .await
            .unwrap();

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
    }
}
