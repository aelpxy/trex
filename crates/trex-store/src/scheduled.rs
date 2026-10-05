use anyhow::Context;
use sqlx::{FromRow, Row, postgres::PgRow};
use uuid::Uuid;

use crate::{
    Store,
    sessions::{Session, session_columns},
};

macro_rules! columns {
    () => {
        "id, workspace_id, project_id, title, prompt, model, reasoning_effort, schedule, timezone, paused, last_error, \
         EXTRACT(EPOCH FROM next_run_at)::BIGINT AS next_run_at, EXTRACT(EPOCH FROM last_run_at)::BIGINT AS last_run_at, \
         EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at, EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at"
    };
}

// a prompt that runs on a schedule, each run in a new chat
pub struct ScheduledTask {
    pub id: Uuid,
    pub workspace_id: Uuid,
    pub project_id: Option<Uuid>,
    pub title: String,
    pub prompt: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    // a five-field cron expression, evaluated in `timezone`
    pub schedule: String,
    pub timezone: String,
    pub paused: bool,
    // unix seconds; none while paused or when the schedule never matches again
    pub next_run_at: Option<i64>,
    pub last_run_at: Option<i64>,
    // why the last run couldn't start, such as an empty balance
    pub last_error: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl FromRow<'_, PgRow> for ScheduledTask {
    fn from_row(row: &PgRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            workspace_id: row.try_get("workspace_id")?,
            project_id: row.try_get("project_id")?,
            title: row.try_get("title")?,
            prompt: row.try_get("prompt")?,
            model: row.try_get("model")?,
            reasoning_effort: row.try_get("reasoning_effort")?,
            schedule: row.try_get("schedule")?,
            timezone: row.try_get("timezone")?,
            paused: row.try_get("paused")?,
            next_run_at: row.try_get("next_run_at")?,
            last_run_at: row.try_get("last_run_at")?,
            last_error: row.try_get("last_error")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

// what a task is made of; `next_run_at` is unix seconds
pub struct TaskFields<'a> {
    pub project: Option<Uuid>,
    pub title: &'a str,
    pub prompt: &'a str,
    pub model: &'a str,
    pub reasoning_effort: Option<&'a str>,
    pub schedule: &'a str,
    pub timezone: &'a str,
    pub paused: bool,
    pub next_run_at: Option<i64>,
}

impl Store {
    // none when the project isn't the workspace's
    pub async fn create_scheduled_task(
        &self,
        workspace: Uuid,
        task: &TaskFields<'_>,
    ) -> anyhow::Result<Option<ScheduledTask>> {
        let sql = concat!(
            "INSERT INTO scheduled_tasks (id, workspace_id, project_id, title, prompt, model, reasoning_effort, schedule, timezone, paused, next_run_at) ",
            "SELECT $1, $2, $3, $4, $5, $6, $7, $8, $9, $10, TO_TIMESTAMP($11) ",
            "WHERE $3::UUID IS NULL OR EXISTS (SELECT 1 FROM projects WHERE id = $3 AND workspace_id = $2) RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(Uuid::now_v7())
            .bind(workspace)
            .bind(task.project)
            .bind(task.title)
            .bind(task.prompt)
            .bind(task.model)
            .bind(task.reasoning_effort)
            .bind(task.schedule)
            .bind(task.timezone)
            .bind(task.paused)
            .bind(task.next_run_at.map(|at| at as f64))
            .fetch_optional(&self.pg)
            .await
            .context("failed to create scheduled task")
    }

    // replaces every field; none when the task or the project isn't the workspace's
    pub async fn update_scheduled_task(
        &self,
        workspace: Uuid,
        id: Uuid,
        task: &TaskFields<'_>,
    ) -> anyhow::Result<Option<ScheduledTask>> {
        let sql = concat!(
            "UPDATE scheduled_tasks SET project_id = $3, title = $4, prompt = $5, model = $6, reasoning_effort = $7, schedule = $8, ",
            "timezone = $9, paused = $10, next_run_at = TO_TIMESTAMP($11), updated_at = NOW() ",
            "WHERE id = $1 AND workspace_id = $2 ",
            "AND ($3::UUID IS NULL OR EXISTS (SELECT 1 FROM projects WHERE id = $3 AND workspace_id = $2)) RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(workspace)
            .bind(task.project)
            .bind(task.title)
            .bind(task.prompt)
            .bind(task.model)
            .bind(task.reasoning_effort)
            .bind(task.schedule)
            .bind(task.timezone)
            .bind(task.paused)
            .bind(task.next_run_at.map(|at| at as f64))
            .fetch_optional(&self.pg)
            .await
            .context("failed to update scheduled task")
    }

    pub async fn scheduled_tasks(&self, workspace: Uuid) -> anyhow::Result<Vec<ScheduledTask>> {
        let sql = concat!(
            "SELECT ",
            columns!(),
            " FROM scheduled_tasks WHERE workspace_id = $1 ORDER BY created_at DESC"
        );
        sqlx::query_as(sql)
            .bind(workspace)
            .fetch_all(&self.pg)
            .await
            .context("failed to list scheduled tasks")
    }

    pub async fn scheduled_task(
        &self,
        workspace: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Option<ScheduledTask>> {
        let sql = concat!(
            "SELECT ",
            columns!(),
            " FROM scheduled_tasks WHERE id = $1 AND workspace_id = $2"
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(workspace)
            .fetch_optional(&self.pg)
            .await
            .context("failed to load scheduled task")
    }

    pub async fn count_scheduled_tasks(&self, workspace: Uuid) -> anyhow::Result<i64> {
        sqlx::query_scalar("SELECT COUNT(*) FROM scheduled_tasks WHERE workspace_id = $1")
            .bind(workspace)
            .fetch_one(&self.pg)
            .await
            .context("failed to count scheduled tasks")
    }

    // its past runs keep their chats
    pub async fn delete_scheduled_task(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM scheduled_tasks WHERE id = $1 AND workspace_id = $2")
            .bind(id)
            .bind(workspace)
            .execute(&self.pg)
            .await
            .context("failed to delete scheduled task")?;
        Ok(result.rows_affected() > 0)
    }

    // takes the tasks that are due and moves each to its next run in the same transaction, so every
    // instance can tick without two of them starting the same run; `next` gives the following run
    // tasks wait while every member of their workspace is suspended, then run once
    pub async fn claim_due_tasks(
        &self,
        limit: i64,
        next: impl Fn(&ScheduledTask) -> Option<i64>,
    ) -> anyhow::Result<Vec<ScheduledTask>> {
        let mut tx = self
            .pg
            .begin()
            .await
            .context("failed to start a transaction")?;
        let sql = concat!(
            "SELECT ",
            columns!(),
            " FROM scheduled_tasks WHERE NOT paused AND next_run_at <= NOW() \
             AND EXISTS (SELECT 1 FROM workspace_members m JOIN users u ON u.id = m.user_id \
             WHERE m.workspace_id = scheduled_tasks.workspace_id AND u.suspended_at IS NULL) \
             ORDER BY next_run_at LIMIT $1 FOR UPDATE SKIP LOCKED"
        );
        let due: Vec<ScheduledTask> = sqlx::query_as(sql)
            .bind(limit)
            .fetch_all(&mut *tx)
            .await
            .context("failed to find due tasks")?;
        for task in &due {
            sqlx::query("UPDATE scheduled_tasks SET next_run_at = TO_TIMESTAMP($2), last_run_at = NOW() WHERE id = $1")
                .bind(task.id)
                .bind(next(task).map(|at| at as f64))
                .execute(&mut *tx)
                .await
                .context("failed to schedule the next run")?;
        }
        tx.commit().await.context("failed to claim due tasks")?;
        Ok(due)
    }

    // the outcome of starting a run: none clears an earlier error
    pub async fn record_task_run(&self, id: Uuid, error: Option<&str>) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE scheduled_tasks SET last_error = $2, last_run_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(error)
        .execute(&self.pg)
        .await
        .context("failed to record the task run")?;
        Ok(())
    }

    // the chat a run happens in, titled after the task and linked to it
    pub async fn create_task_session(
        &self,
        task: &ScheduledTask,
        title: &str,
    ) -> anyhow::Result<Session> {
        let sql = concat!(
            "INSERT INTO sessions (id, workspace_id, project_id, scheduled_task_id, title, model, reasoning_effort, fast) ",
            "VALUES ($1, $2, $3, $4, $5, $6, $7, FALSE) RETURNING ",
            session_columns!()
        );
        sqlx::query_as(sql)
            .bind(Uuid::now_v7())
            .bind(task.workspace_id)
            .bind(task.project_id)
            .bind(task.id)
            .bind(title)
            .bind(&task.model)
            .bind(task.reasoning_effort.as_deref())
            .fetch_one(&self.pg)
            .await
            .context("failed to create the run's chat")
    }

    // what the previous run said, for the next run's context
    pub async fn last_task_reply(&self, task: Uuid) -> anyhow::Result<Option<(i64, String)>> {
        let row = sqlx::query(
            "SELECT EXTRACT(EPOCH FROM s.created_at)::BIGINT AS at, i.item FROM sessions s \
             JOIN session_items i ON i.session_id = s.id \
             WHERE s.scheduled_task_id = $1 AND i.item->>'role' = 'assistant' \
             ORDER BY s.created_at DESC, i.seq DESC LIMIT 1",
        )
        .bind(task)
        .fetch_optional(&self.pg)
        .await
        .context("failed to load the previous run")?;
        let Some(row) = row else { return Ok(None) };
        let item: serde_json::Value = row.try_get("item")?;
        let text = item["content"]
            .as_array()
            .map(|parts| {
                parts
                    .iter()
                    .filter_map(|part| part["text"].as_str())
                    .collect::<Vec<_>>()
                    .join("")
            })
            .unwrap_or_default();
        Ok(Some((row.try_get("at")?, text)))
    }
}

#[cfg(test)]
mod tests {
    use crate::Store;

    // needs Postgres from .env; a limit of 0 checks the query without taking the dev server's tasks
    #[tokio::test]
    #[ignore]
    async fn claims_due_tasks() {
        dotenvy::dotenv().ok();
        let store = Store::connect(
            &std::env::var("TREX_DATABASE_URL").unwrap(),
            &std::env::var("TREX_REDIS_URL").unwrap(),
        )
        .await
        .unwrap();
        assert!(store.claim_due_tasks(0, |_| None).await.unwrap().is_empty());
    }
}
