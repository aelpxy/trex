use std::time::Duration;

use anyhow::Context;
use sqlx::{Postgres, Row, Transaction};
use uuid::Uuid;

use crate::Store;

pub struct LiveSandbox {
    pub session: Uuid,
    pub workspace: Uuid,
    pub sandbox: String,
    pub auto_approve: bool,
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

impl Store {
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

    // sandboxes that are up between runs, where background processes may still be denied access;
    // runs watch their own
    pub async fn live_sandboxes(&self, idle: Duration) -> anyhow::Result<Vec<LiveSandbox>> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, sandbox, auto_approve FROM sessions \
             WHERE sandbox IS NOT NULL AND NOT sandbox_stopped AND status <> 'running' \
             AND updated_at >= NOW() - MAKE_INTERVAL(secs => $1)",
        )
        .bind(idle.as_secs_f64())
        .fetch_all(&self.pg)
        .await
        .context("failed to list live sandboxes")?;
        rows.iter()
            .map(|row| {
                Ok(LiveSandbox {
                    session: row.try_get("id")?,
                    workspace: row.try_get("workspace_id")?,
                    sandbox: row.try_get("sandbox")?,
                    auto_approve: row.try_get("auto_approve")?,
                })
            })
            .collect()
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
}
