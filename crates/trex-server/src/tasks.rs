use chrono::Utc;
use futures::future::BoxFuture;
use trex_harness::tool::{CreatedTask, NewTask, TaskScheduler};
use trex_store::{
    scheduled::{ScheduledTask, TaskFields},
    sessions::Session,
};
use uuid::Uuid;

use crate::{
    api::{AppState, error::ApiError, sessions::check_settings},
    runs,
    schedule::Schedule,
};

const MAX_TASKS: i64 = 20;
const MAX_TITLE_CHARS: usize = 100;
const MAX_PROMPT_CHARS: usize = 20_000;

// a task's settings, checked and with its next run worked out
pub struct TaskSettings {
    pub project: Option<Uuid>,
    pub title: String,
    pub prompt: String,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub schedule: String,
    pub timezone: String,
    pub paused: bool,
}

impl TaskSettings {
    pub async fn check(self, state: &AppState, workspace: Uuid) -> Result<Self, ApiError> {
        let title = self.title.trim().to_owned();
        if title.is_empty() || title.chars().count() > MAX_TITLE_CHARS {
            return Err(ApiError::invalid(
                format!("title must be 1 to {MAX_TITLE_CHARS} characters"),
                "title",
            ));
        }
        let prompt = self.prompt.trim().to_owned();
        if prompt.is_empty() || prompt.chars().count() > MAX_PROMPT_CHARS {
            return Err(ApiError::invalid(
                format!("prompt must be 1 to {MAX_PROMPT_CHARS} characters"),
                "prompt",
            ));
        }
        check_settings(state, &self.model, self.reasoning_effort.as_deref(), false)?;
        runs::require_model(state, workspace, &self.model).await?;
        Schedule::parse(&self.schedule, &self.timezone)
            .map_err(|error| ApiError::invalid(error, "schedule"))?;
        Ok(Self {
            title,
            prompt,
            schedule: self.schedule.trim().to_owned(),
            ..self
        })
    }

    pub fn fields(&self) -> TaskFields<'_> {
        let next_run_at = (!self.paused)
            .then(|| {
                Schedule::parse(&self.schedule, &self.timezone)
                    .ok()?
                    .next_after(Utc::now())
            })
            .flatten()
            .map(|at| at.timestamp());
        TaskFields {
            project: self.project,
            title: &self.title,
            prompt: &self.prompt,
            model: &self.model,
            reasoning_effort: self.reasoning_effort.as_deref(),
            schedule: &self.schedule,
            timezone: &self.timezone,
            paused: self.paused,
            next_run_at,
        }
    }
}

// a new task in `workspace`, from the api or the agent's schedule_task tool
pub async fn create(
    state: &AppState,
    workspace: Uuid,
    settings: TaskSettings,
) -> Result<ScheduledTask, ApiError> {
    if state.store.count_scheduled_tasks(workspace).await? >= MAX_TASKS {
        return Err(ApiError::Conflict(format!(
            "a workspace can have up to {MAX_TASKS} scheduled tasks"
        )));
    }
    let valid = settings.check(state, workspace).await?;
    state
        .store
        .create_scheduled_task(workspace, &valid.fields())
        .await?
        .ok_or_else(|| ApiError::NotFound("no such project".into()))
}

// the agent's schedule_task tool, creating tasks that run like the chat it was called from
pub struct ChatScheduler<'a> {
    pub state: &'a AppState,
    pub workspace: Uuid,
    pub session: &'a Session,
}

impl TaskScheduler for ChatScheduler<'_> {
    fn schedule(&self, task: NewTask) -> BoxFuture<'_, anyhow::Result<CreatedTask>> {
        Box::pin(async move {
            let settings = TaskSettings {
                project: self.session.project_id,
                title: task.title,
                prompt: task.prompt,
                model: self.session.model.clone(),
                reasoning_effort: self.session.reasoning_effort.clone(),
                schedule: task.schedule,
                timezone: task.timezone,
                paused: false,
            };
            // the model reads the reason, so it can fix the schedule and try again
            let created = create(self.state, self.workspace, settings)
                .await
                .map_err(|error| anyhow::anyhow!(error.message()))?;
            tracing::info!(workspace = %self.workspace, session = %self.session.id, task = %created.id, "agent scheduled a task");
            Ok(CreatedTask {
                title: created.title,
                next_run_at: created.next_run_at,
            })
        })
    }
}
