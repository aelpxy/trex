use std::time::Duration;

use anyhow::Context;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use super::SessionStatus;
use crate::Store;

pub struct StaleRun {
    pub session: Uuid,
    pub workspace: Uuid,
    pub run: Uuid,
}

// what a run learns when it renews its lease
#[derive(Debug, PartialEq)]
pub enum Lease {
    Held,
    // someone asked it to stop, maybe through another instance
    CancelRequested,
    // another instance took the run over
    Lost,
}

#[derive(Debug, PartialEq)]
pub enum Finish {
    Finished,
    // messages arrived as the run ended; it has to read them before it can finish
    MessagesQueued,
    // another instance took the run over
    NotOwner,
}

impl Store {
    // the conditional update is what guarantees one run per session, even across trex instances;
    // `run` owns the session until it finishes or its heartbeat goes stale
    pub async fn start_run(&self, workspace: Uuid, id: Uuid, run: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET status = 'running', pending_question = NULL, last_error = NULL, \
             run_id = $3, run_heartbeat_at = NOW(), run_started_at = NOW(), run_cancel_requested = FALSE, updated_at = NOW() \
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

    pub async fn heartbeat_run(&self, id: Uuid, run: Uuid) -> anyhow::Result<Lease> {
        let cancel: Option<bool> = sqlx::query_scalar(
            "UPDATE sessions SET run_heartbeat_at = NOW() WHERE id = $1 AND run_id = $2 AND status = 'running' \
             RETURNING run_cancel_requested",
        )
        .bind(id)
        .bind(run)
        .fetch_optional(&self.pg)
        .await
        .context("failed to renew run lease")?;
        Ok(match cancel {
            None => Lease::Lost,
            Some(true) => Lease::CancelRequested,
            Some(false) => Lease::Held,
        })
    }

    // asks the session's run to stop; whichever instance holds it sees this at its next heartbeat.
    // `workspace` is none only for admins. false when nothing is running
    pub async fn request_cancel(&self, workspace: Option<Uuid>, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE sessions SET run_cancel_requested = TRUE \
             WHERE id = $1 AND ($2::UUID IS NULL OR workspace_id = $2) AND status = 'running'",
        )
        .bind(id)
        .bind(workspace)
        .execute(&self.pg)
        .await
        .context("failed to request cancel")?;
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
             run_heartbeat_at = NULL, run_started_at = NULL, run_cancel_requested = FALSE, updated_at = NOW() \
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
             WHERE id = $1 AND workspace_id = $2 AND status = 'running' AND NOT run_cancel_requested",
        )
        .bind(id)
        .bind(workspace)
        .bind(item)
        .execute(&self.pg)
        .await
        .context("failed to queue message")?;
        Ok(result.rows_affected() > 0)
    }

    // a run's leftover messages, only while `run` still holds the session; another instance that took
    // the run over reads them itself
    pub async fn take_run_leftovers(
        &self,
        workspace: Uuid,
        id: Uuid,
        run: Uuid,
    ) -> anyhow::Result<Vec<Value>> {
        let queued: Option<Value> = sqlx::query_scalar(
            "UPDATE sessions s SET queued_messages = '[]'::JSONB \
             FROM (SELECT id, queued_messages FROM sessions WHERE id = $1 AND workspace_id = $2 \
             AND run_id = $3 AND status = 'running' FOR UPDATE) old \
             WHERE s.id = old.id RETURNING old.queued_messages",
        )
        .bind(id)
        .bind(workspace)
        .bind(run)
        .fetch_optional(&self.pg)
        .await
        .context("failed to take leftover messages")?;
        queued
            .map(serde_json::from_value)
            .transpose()
            .context("invalid queued messages")
            .map(Option::unwrap_or_default)
    }

    // whether another run holds the session now, as opposed to `run` having finished it
    pub async fn run_taken_over(&self, id: Uuid, run: Uuid) -> anyhow::Result<bool> {
        let taken: Option<bool> = sqlx::query_scalar(
            "SELECT status = 'running' AND run_id IS DISTINCT FROM $2 FROM sessions WHERE id = $1",
        )
        .bind(id)
        .bind(run)
        .fetch_optional(&self.pg)
        .await
        .context("failed to check run owner")?;
        // a deleted session has no run to finish either
        Ok(taken.unwrap_or(true))
    }

    // runs in these workspaces whose instance is still alive, i.e. renewed within `stale_after`
    pub async fn active_runs(
        &self,
        workspaces: &[Uuid],
        stale_after: Duration,
    ) -> anyhow::Result<i64> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM sessions WHERE workspace_id = ANY($1) AND status = 'running' \
             AND run_heartbeat_at > NOW() - MAKE_INTERVAL(secs => $2)",
        )
        .bind(workspaces)
        .bind(stale_after.as_secs_f64())
        .fetch_one(&self.pg)
        .await
        .context("failed to count active runs")
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
}
