mod items;
mod runs;
mod sandboxes;

use anyhow::{Context, bail};
use serde_json::Value;
use sqlx::{FromRow, Row, postgres::PgRow};
use uuid::Uuid;

use crate::Store;

pub use self::{
    items::{UsageEntry, UsageRecord},
    runs::{Finish, Lease, StaleRun},
    sandboxes::{IdleSandbox, LiveSandbox},
};

// sqlx only accepts static sql, so the shared column list is spliced in at compile time
macro_rules! columns {
    () => {
        "id, workspace_id, project_id, scheduled_task_id, title, model, reasoning_effort, fast, auto_approve, sandbox, sandbox_stopped, status, pending_question, last_error, queued_messages, \
         EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at, EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at"
    };
}
pub(crate) use columns as session_columns;

// the chats people start; a scheduled task's runs are only listed under that task
#[derive(Clone, Copy)]
pub enum SessionFilter {
    All,
    NoProject,
    Project(Uuid),
    ScheduledTask(Uuid),
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
    // set on the chats a scheduled task's runs create
    pub scheduled_task_id: Option<Uuid>,
    pub title: Option<String>,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub fast: bool,
    // network access requests are approved without asking the user
    pub auto_approve: bool,
    pub sandbox: Option<String>,
    pub sandbox_stopped: bool,
    pub status: SessionStatus,
    pub pending_question: Option<Value>,
    pub last_error: Option<String>,
    // messages sent during the current run that the agent hasn't read yet
    pub queued_messages: Vec<Value>,
    pub created_at: i64,
    pub updated_at: i64,
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
            scheduled_task_id: row.try_get("scheduled_task_id")?,
            title: row.try_get("title")?,
            model: row.try_get("model")?,
            reasoning_effort: row.try_get("reasoning_effort")?,
            fast: row.try_get("fast")?,
            auto_approve: row.try_get("auto_approve")?,
            sandbox: row.try_get("sandbox")?,
            sandbox_stopped: row.try_get("sandbox_stopped")?,
            status: SessionStatus::parse(&status)
                .map_err(|error| sqlx::Error::Decode(error.into()))?,
            pending_question: row.try_get("pending_question")?,
            last_error: row.try_get("last_error")?,
            queued_messages: row
                .try_get::<sqlx::types::Json<Vec<Value>>, _>("queued_messages")?
                .0,
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
            "INSERT INTO sessions (id, workspace_id, project_id, title, model, reasoning_effort, fast, auto_approve, reasoning_model, reasoning_from) ",
            "SELECT $1, workspace_id, project_id, title, model, reasoning_effort, fast, auto_approve, reasoning_model, LEAST(reasoning_from, $4) ",
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
            "AND (($4 = 'task' AND scheduled_task_id = $5) OR ($4 <> 'task' AND scheduled_task_id IS NULL AND ",
            "($4 = 'all' OR ($4 = 'none' AND project_id IS NULL) OR project_id = $5))) ORDER BY id DESC LIMIT $3"
        );
        let (kind, id) = match filter {
            SessionFilter::All => ("all", None),
            SessionFilter::NoProject => ("none", None),
            SessionFilter::Project(project) => ("project", Some(project)),
            SessionFilter::ScheduledTask(task) => ("task", Some(task)),
        };
        sqlx::query_as(sql)
            .bind(workspace)
            .bind(before)
            .bind(limit)
            .bind(kind)
            .bind(id)
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

    pub async fn set_session_auto_approve(
        &self,
        workspace: Uuid,
        id: Uuid,
        auto_approve: bool,
    ) -> anyhow::Result<Option<Session>> {
        let sql = concat!(
            "UPDATE sessions SET auto_approve = $3, updated_at = NOW() WHERE id = $1 AND workspace_id = $2 RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(workspace)
            .bind(auto_approve)
            .fetch_optional(&self.pg)
            .await
            .context("failed to update session auto approve")
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
}

#[cfg(test)]
mod tests;
