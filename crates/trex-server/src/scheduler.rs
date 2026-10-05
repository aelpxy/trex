use std::{sync::Arc, time::Duration};

use chrono::{TimeZone, Utc};
use chrono_tz::Tz;
use serde_json::json;
use tokio::time::interval;
use trex_harness::history;
use trex_store::{scheduled::ScheduledTask, sessions::Session};

use crate::{
    api::{AppState, error::ApiError},
    credits, runs,
    schedule::Schedule,
};

const TICK: Duration = Duration::from_secs(30);
// tasks started per tick; more due tasks wait for the next one
const BATCH: i64 = 20;
// how much of the previous run's reply the next run sees
const PREVIOUS_REPLY_CHARS: usize = 1500;

// starts due tasks until the server stops; every instance ticks, and claiming keeps runs unique.
// a missed slot, say while the server was down, runs once on the next tick rather than once per slot
pub async fn run_due_tasks(state: Arc<AppState>) {
    let mut ticks = interval(TICK);
    loop {
        tokio::select! {
            _ = ticks.tick() => {}
            () = state.shutdown.cancelled() => return,
        }
        let claimed = state
            .store
            .claim_due_tasks(BATCH, |task| next_run(task, Utc::now()))
            .await;
        let tasks = match claimed {
            Ok(tasks) => tasks,
            Err(error) => {
                tracing::warn!(
                    error = format!("{error:#}"),
                    "failed to claim due scheduled tasks"
                );
                continue;
            }
        };
        for task in tasks {
            if let Err(error) = start(&state, &task).await {
                tracing::info!(task = %task.id, error = error.message(), "scheduled task didn't run");
            }
        }
    }
}

// when a task runs next, in unix seconds; none when paused or its schedule no longer parses
pub fn next_run(task: &ScheduledTask, after: chrono::DateTime<Utc>) -> Option<i64> {
    if task.paused {
        return None;
    }
    Schedule::parse(&task.schedule, &task.timezone)
        .ok()?
        .next_after(after)
        .map(|at| at.timestamp())
}

// runs the task now in a new chat; why it couldn't start is recorded on the task
pub async fn start(state: &Arc<AppState>, task: &ScheduledTask) -> Result<Session, ApiError> {
    let started = start_run(state, task).await;
    let error = started.as_ref().err().map(ApiError::message);
    // an internal error's details stay in the log, not on the task
    if let Err(ApiError::Internal(cause)) = &started {
        tracing::error!(task = %task.id, error = format!("{cause:#}"), "failed to start a scheduled run");
    }
    if let Err(record) = state.store.record_task_run(task.id, error.as_deref()).await {
        tracing::warn!(task = %task.id, error = format!("{record:#}"), "failed to record the scheduled run");
    }
    started
}

async fn start_run(state: &Arc<AppState>, task: &ScheduledTask) -> Result<Session, ApiError> {
    // checked before the chat exists, so a run that can't start leaves no empty chat behind
    runs::require_model(state, task.workspace_id, &task.model).await?;
    credits::require(state, task.workspace_id).await?;
    let mut input = Vec::new();
    if let Some(note) = previous_run_note(state, task).await? {
        input.push(json!({"type": "message", "role": "developer", "content": note}));
    }
    input.extend(history::to_json(&[history::user_message(&task.prompt)])?);
    let session = state
        .store
        .create_task_session(task, &run_title(task))
        .await?;
    runs::start(state, task.workspace_id, &session, input).await?;
    Ok(session)
}

// each run is a new chat, so the model hears how the last one went instead of seeing its history
async fn previous_run_note(
    state: &AppState,
    task: &ScheduledTask,
) -> Result<Option<String>, ApiError> {
    let Some((at, reply)) = state.store.last_task_reply(task.id).await? else {
        return Ok(None);
    };
    let when = local_time(task, at).unwrap_or_default();
    let reply: String = reply.chars().take(PREVIOUS_REPLY_CHARS).collect();
    Ok(Some(format!(
        "This is a scheduled run of the task \"{}\"; no one is watching, so don't ask questions. \
         Its previous run ({when}) ended with:\n<previous_reply>\n{reply}\n</previous_reply>",
        task.title
    )))
}

fn run_title(task: &ScheduledTask) -> String {
    match local_time(task, Utc::now().timestamp()) {
        Some(when) => format!("{} · {when}", task.title),
        None => task.title.clone(),
    }
}

fn local_time(task: &ScheduledTask, seconds: i64) -> Option<String> {
    let timezone: Tz = task.timezone.parse().ok()?;
    let at = Utc
        .timestamp_opt(seconds, 0)
        .single()?
        .with_timezone(&timezone);
    Some(at.format("%b %-d, %H:%M").to_string())
}
