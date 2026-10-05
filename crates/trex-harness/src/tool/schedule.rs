use anyhow::Context;
use async_openai::types::responses::FunctionTool;
use chrono::DateTime;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext};

pub const SCHEDULE_TASK: &str = "schedule_task";

// what the model asks for; the chat's model, effort and project come from the caller
pub struct NewTask {
    pub title: String,
    pub prompt: String,
    pub schedule: String,
    pub timezone: String,
}

pub struct CreatedTask {
    pub title: String,
    // unix seconds
    pub next_run_at: Option<i64>,
}

// creates tasks in the run's workspace; the server implements it with its own validation
pub trait TaskScheduler: Send + Sync {
    fn schedule(&self, task: NewTask) -> BoxFuture<'_, anyhow::Result<CreatedTask>>;
}

pub struct ScheduleTask;

#[derive(Deserialize)]
struct Args {
    title: String,
    prompt: String,
    schedule: String,
    timezone: String,
}

impl Tool for ScheduleTask {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: SCHEDULE_TASK.into(),
            description: Some(
                "Schedule a prompt to run by itself on a recurring schedule, such as a daily digest or a weekly check. \
                 Each run starts a new chat with this chat's model and project, works without asking questions, and \
                 can't see this conversation. Use it only when the user asks for something to happen regularly, and \
                 say in your reply when it will run. At most hourly."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "title": {"type": "string", "description": "A short name for the task, up to 100 characters."},
                    "prompt": {"type": "string", "description": "Complete instructions for each run, since it starts without this conversation: what to do, where to find things, and what to report."},
                    "schedule": {"type": "string", "description": "A five-field cron expression (minute hour day-of-month month day-of-week), e.g. \"0 9 * * 1-5\" for 9:00 on weekdays."},
                    "timezone": {"type": "string", "description": "The IANA timezone the schedule is read in, e.g. Europe/Berlin; the user's if they said it, otherwise ask them or use UTC."}
                },
                "required": ["title", "prompt", "schedule", "timezone"],
                "additionalProperties": false,
            })),
            strict: Some(true),
            ..Default::default()
        }
    }

    fn call<'a>(
        &'a self,
        ctx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, anyhow::Result<String>> {
        Box::pin(async move {
            let args: Args = serde_json::from_value(args)?;
            let scheduler = ctx
                .scheduler
                .context("tasks can't be scheduled from this run")?;
            let created = scheduler
                .schedule(NewTask {
                    title: args.title,
                    prompt: args.prompt,
                    schedule: args.schedule,
                    timezone: args.timezone,
                })
                .await?;
            Ok(describe(&created))
        })
    }
}

// the next run is given in UTC; the model converts it for the user with the timezone it chose
fn describe(created: &CreatedTask) -> String {
    let next = created
        .next_run_at
        .and_then(|at| DateTime::from_timestamp(at, 0))
        .map_or_else(
            || "not scheduled".to_owned(),
            |at| at.format("%Y-%m-%d %H:%M UTC").to_string(),
        );
    format!(
        "Scheduled \"{}\". Next run: {next}. Each run is a new chat; the user can pause, edit or delete it on the Scheduled page.",
        created.title
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reports_the_next_run() {
        let created = CreatedTask {
            title: "Morning digest".into(),
            next_run_at: Some(1_800_000_000),
        };
        assert_eq!(
            describe(&created),
            "Scheduled \"Morning digest\". Next run: 2027-01-15 08:00 UTC. Each run is a new chat; the user can pause, edit or delete it on the Scheduled page."
        );
    }
}
