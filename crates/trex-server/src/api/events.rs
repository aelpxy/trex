use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::sse::{Event as SseEvent, KeepAlive, Sse},
};
use futures::{Stream, StreamExt, stream};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use trex_harness::event::{Event, FileChange, OutputStream, StepStatus as HarnessStepStatus};
use utoipa::{IntoParams, ToSchema};

use super::{
    AppState,
    auth::Auth,
    error::{ApiError, ErrorResponse},
    sessions::{Question, find_session},
};

const READ_BLOCK: Duration = Duration::from_secs(15);
const KEEP_ALIVE: Duration = Duration::from_secs(15);

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EventsQuery {
    /// `start` replays every retained event, `run` replays from the latest run's start; otherwise
    /// the stream starts at the live tail.
    #[param(example = "start")]
    from: Option<String>,
}

/// An event on a session's stream. The SSE `event:` name equals `type`.
#[derive(Serialize, ToSchema)]
#[serde(tag = "type")]
pub enum SessionEvent {
    /// `items` and `usage` are how many conversation items (by `seq`) and usage records existed when
    /// the run started; a client that reloads mid-run keeps those and rebuilds the rest from this
    /// run's events (`?from=run`).
    /// `compact` marks a run that only compacts the context, which the user asked for.
    #[serde(rename = "run.started")]
    RunStarted {
        items: i64,
        usage: i64,
        compact: bool,
    },
    /// The chat was given a title, generated from its first message.
    #[serde(rename = "session.updated")]
    SessionUpdated { title: String },
    /// trex restarted while the run was in progress and has picked it up again from saved history,
    /// counted by `items` and `usage` as in `run.started`. Tool calls without a result were
    /// interrupted; a response that was still streaming is generated again. Follows the run's
    /// `run.started` in place of its end.
    #[serde(rename = "run.resumed")]
    RunResumed { items: i64, usage: i64 },
    #[serde(rename = "sandbox.creating")]
    SandboxCreating,
    /// The conversation's stopped sandbox is starting again, with its files intact.
    #[serde(rename = "sandbox.starting")]
    SandboxStarting,
    #[serde(rename = "sandbox.ready")]
    SandboxReady,
    /// The conversation's sandbox was lost and replaced with an empty one; files, installs and
    /// processes from before are gone.
    #[serde(rename = "sandbox.replaced")]
    SandboxReplaced { reason: String },
    #[serde(rename = "text.delta")]
    TextDelta { delta: String },
    #[serde(rename = "reasoning.delta")]
    ReasoningDelta { delta: String },
    /// The model began writing a tool call; `tool.call` follows once its arguments are complete.
    #[serde(rename = "tool.call.started")]
    ToolCallStarted { call_id: String, name: String },
    /// A piece of a started tool call's JSON-encoded arguments.
    #[serde(rename = "tool.call.delta")]
    ToolCallDelta { call_id: String, delta: String },
    #[serde(rename = "tool.call")]
    ToolCall {
        call_id: String,
        name: String,
        /// JSON-encoded arguments.
        arguments: String,
    },
    /// Live output of a running command.
    #[serde(rename = "tool.output")]
    ToolOutput {
        call_id: String,
        stream: ToolStream,
        chunk: String,
    },
    #[serde(rename = "tool.result")]
    ToolResult {
        call_id: String,
        output: String,
        is_error: bool,
    },
    /// Token usage of one model response.
    #[serde(rename = "usage")]
    Usage {
        model: String,
        input_tokens: u64,
        cached_input_tokens: u64,
        cache_write_tokens: u64,
        output_tokens: u64,
        reasoning_tokens: u64,
        /// From sending the request to the end of the response.
        duration_ms: u64,
        /// Until the first streamed text, reasoning or tool arguments.
        time_to_first_token_ms: Option<u64>,
        /// What the response cost.
        credits: i64,
    },
    /// The sandbox was denied network access; see the access request endpoints.
    #[serde(rename = "access.requested")]
    AccessRequested {
        id: String,
        endpoints: Vec<String>,
        binary: String,
        rationale: String,
        security_notes: String,
    },
    /// The agent asked the user something; the run ends with `run.needs_input`.
    #[serde(rename = "question")]
    Question { questions: Vec<Question> },
    /// A model request failed transiently and will be retried after `delay_ms`. Discard the text,
    /// reasoning and tool calls streamed since the last `tool.result` (or the run start).
    #[serde(rename = "model.retrying")]
    ModelRetrying {
        attempt: u32,
        max_attempts: u32,
        delay_ms: u64,
        reason: String,
    },
    /// A tool created, changed, moved or deleted a file in the sandbox. `diff` is a unified diff
    /// of the file (cut short when huge); for a move, `from` is the old path.
    #[serde(rename = "file.changed")]
    FileChanged {
        /// The tool call that changed the file.
        call_id: String,
        path: String,
        change: FileChangeKind,
        from: Option<String>,
        diff: String,
        /// The whole file before the change, when it existed and is at most 64 KiB.
        before: Option<String>,
        /// The whole file after the change, unless it was deleted or is over 64 KiB.
        after: Option<String>,
    },
    /// The agent's plan for the task, replacing any earlier one. Render it as a checklist.
    /// The agent wants the user to see a server running in the sandbox; create a preview for
    /// `port` (`POST /v1/sessions/{id}/previews`) and open it at `path`.
    #[serde(rename = "preview.opened")]
    PreviewOpened { port: u16, path: String },
    #[serde(rename = "plan.updated")]
    PlanUpdated {
        explanation: Option<String>,
        steps: Vec<PlanStep>,
    },
    /// The context is being summarized, because it's nearly full or the user asked; this can take
    /// a while.
    #[serde(rename = "context.compacting")]
    ContextCompacting,
    /// Later requests start from `summary`; it appears as a `compaction` item.
    #[serde(rename = "context.compacted")]
    ContextCompacted { summary: String },
    /// A message sent while the agent was working is now part of the conversation, after
    /// everything streamed so far. The agent reads it before its next step.
    #[serde(rename = "message.received")]
    MessageReceived { content: String },
    /// The user interrupted the agent with a message. Discard the text and reasoning streamed
    /// since the last `tool.result`; tool calls without a result were stopped. The run continues.
    #[serde(rename = "run.interrupted")]
    RunInterrupted,
    #[serde(rename = "run.completed")]
    RunCompleted,
    /// Waiting for answers to the session's `pending_questions`.
    #[serde(rename = "run.needs_input")]
    RunNeedsInput,
    #[serde(rename = "run.cancelled")]
    RunCancelled,
    /// `code` is `insufficient_credits` when the workspace ran out of credits.
    #[serde(rename = "run.failed")]
    RunFailed {
        error: String,
        code: Option<&'static str>,
    },
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum FileChangeKind {
    Added,
    Updated,
    Deleted,
    Moved,
}

#[derive(Serialize, ToSchema)]
pub struct PlanStep {
    step: String,
    status: StepStatus,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
}

#[derive(Serialize, ToSchema)]
#[serde(rename_all = "snake_case")]
pub enum ToolStream {
    Stdout,
    Stderr,
}

// harness events become api events here; anything internal (review tokens, raw items) stays out
pub fn to_api(event: Event) -> Option<SessionEvent> {
    let event = match event {
        Event::SandboxCreating => SessionEvent::SandboxCreating,
        Event::SandboxStarting => SessionEvent::SandboxStarting,
        Event::SandboxReady => SessionEvent::SandboxReady,
        Event::SandboxReplaced { reason } => SessionEvent::SandboxReplaced { reason },
        Event::TextDelta { delta } => SessionEvent::TextDelta { delta },
        Event::ReasoningDelta { delta } => SessionEvent::ReasoningDelta { delta },
        Event::ToolCallStarted { call_id, name } => SessionEvent::ToolCallStarted { call_id, name },
        Event::ToolCallDelta { call_id, delta } => SessionEvent::ToolCallDelta { call_id, delta },
        Event::ToolCall {
            call_id,
            name,
            arguments,
        } => SessionEvent::ToolCall {
            call_id,
            name,
            arguments,
        },
        Event::ToolOutput {
            call_id,
            stream,
            chunk,
        } => SessionEvent::ToolOutput {
            call_id,
            stream: match stream {
                OutputStream::Stdout => ToolStream::Stdout,
                OutputStream::Stderr => ToolStream::Stderr,
            },
            chunk,
        },
        Event::ToolResult {
            call_id,
            output,
            is_error,
        } => SessionEvent::ToolResult {
            call_id,
            output,
            is_error,
        },
        Event::Usage(usage) => SessionEvent::Usage {
            model: usage.model,
            input_tokens: usage.input_tokens,
            cached_input_tokens: usage.cached_input_tokens,
            cache_write_tokens: usage.cache_write_tokens,
            output_tokens: usage.output_tokens,
            reasoning_tokens: usage.reasoning_tokens,
            duration_ms: usage.duration.as_millis() as u64,
            time_to_first_token_ms: usage
                .time_to_first_token
                .map(|elapsed| elapsed.as_millis() as u64),
            credits: 0,
        },
        Event::AccessRequest(request) => SessionEvent::AccessRequested {
            id: request.id,
            endpoints: request.endpoints,
            binary: request.binary,
            rationale: request.rationale,
            security_notes: request.security_notes,
        },
        Event::Question { questions, .. } => SessionEvent::Question {
            questions: questions.into_iter().map(Question::from).collect(),
        },
        Event::Retrying {
            attempt,
            max_attempts,
            delay,
            reason,
        } => SessionEvent::ModelRetrying {
            attempt,
            max_attempts,
            delay_ms: delay.as_millis() as u64,
            reason,
        },
        Event::FileChanged {
            call_id,
            path,
            change,
            diff,
            before,
            after,
        } => {
            let (change, from) = match change {
                FileChange::Added => (FileChangeKind::Added, None),
                FileChange::Updated => (FileChangeKind::Updated, None),
                FileChange::Deleted => (FileChangeKind::Deleted, None),
                FileChange::Moved { from } => (FileChangeKind::Moved, Some(from)),
            };
            SessionEvent::FileChanged {
                call_id,
                path,
                change,
                from,
                diff,
                before,
                after,
            }
        }
        Event::PreviewOpened { port, path } => SessionEvent::PreviewOpened { port, path },
        Event::PlanUpdated { explanation, steps } => SessionEvent::PlanUpdated {
            explanation,
            steps: steps
                .into_iter()
                .map(|step| PlanStep {
                    step: step.step,
                    status: match step.status {
                        HarnessStepStatus::Pending => StepStatus::Pending,
                        HarnessStepStatus::InProgress => StepStatus::InProgress,
                        HarnessStepStatus::Completed => StepStatus::Completed,
                    },
                })
                .collect(),
        },
        Event::Compacting => SessionEvent::ContextCompacting,
        Event::Compacted { summary } => SessionEvent::ContextCompacted { summary },
        Event::MessageReceived { content } => SessionEvent::MessageReceived { content },
        Event::Interrupted => SessionEvent::RunInterrupted,
        Event::Done => return None,
    };
    Some(event)
}

/// Stream events
///
/// Server-sent events for the session's runs. Reconnect with `Last-Event-ID` to resume without
/// gaps; a comment heartbeat is sent every 15 seconds.
#[utoipa::path(
    get,
    operation_id = "stream_events",
    path = "/sessions/{id}/events",
    tag = "sessions",
    params(
        ("id" = String, Path, description = "Session id"),
        ("Last-Event-ID" = Option<String>, Header, description = "Resume after this event id"),
        EventsQuery,
    ),
    responses(
        (status = 200, content_type = "text/event-stream", body = SessionEvent),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn stream_events(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let resume = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut after = match (resume, query.from.as_deref()) {
        (Some(id), _) => id,
        (None, Some("start")) => "0".to_owned(),
        (None, Some("run")) => match state
            .store
            .run_start(session.id, &["run.started", "run.resumed"])
            .await?
        {
            Some(id) => id,
            None => state
                .store
                .last_event_id(session.id)
                .await?
                .unwrap_or_else(|| "0".to_owned()),
        },
        (None, _) => state
            .store
            .last_event_id(session.id)
            .await?
            .unwrap_or_else(|| "0".to_owned()),
    };
    let mut connection = state.store.event_connection().await?;

    let (tx, rx) = mpsc::channel(64);
    let shutdown = state.shutdown.clone();
    tokio::spawn(async move {
        while !tx.is_closed() {
            let events = match state
                .store
                .read_events(&mut connection, session.id, &after, READ_BLOCK)
                .await
            {
                Ok(events) => events,
                Err(error) => {
                    tracing::warn!(
                        error = format!("{error:#}"),
                        "failed to read session events"
                    );
                    tokio::time::sleep(Duration::from_secs(1)).await;
                    continue;
                }
            };
            for stored in events {
                let kind = stored.event["type"]
                    .as_str()
                    .unwrap_or("message")
                    .to_owned();
                let event = SseEvent::default()
                    .id(&stored.id)
                    .event(kind)
                    .data(stored.event.to_string());
                after = stored.id;
                if tx.send(event).await.is_err() {
                    return;
                }
            }
        }
    });

    // clients reconnect with Last-Event-ID, so ending the stream on shutdown loses nothing
    let stream = stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| (Ok(event), rx))
    })
    .take_until(shutdown.cancelled_owned());
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)))
}
