use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::sse::{Event as SseEvent, KeepAlive, Sse},
};
use futures::{Stream, stream};
use serde::{Deserialize, Serialize};
use tokio::sync::mpsc;
use trex_harness::event::{Event, OutputStream};
use utoipa::{IntoParams, ToSchema};

use super::{
    AppState,
    auth::CurrentUser,
    error::{ApiError, ErrorResponse},
    sessions::{Question, find_session},
};

const READ_BLOCK: Duration = Duration::from_secs(15);
const KEEP_ALIVE: Duration = Duration::from_secs(15);

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct EventsQuery {
    /// `start` replays every retained event; otherwise the stream starts at the live tail.
    #[param(example = "start")]
    from: Option<String>,
}

/// An event on a session's stream. The SSE `event:` name equals `type`.
#[derive(Serialize, ToSchema)]
#[serde(tag = "type")]
pub enum SessionEvent {
    #[serde(rename = "run.started")]
    RunStarted,
    #[serde(rename = "sandbox.creating")]
    SandboxCreating,
    #[serde(rename = "sandbox.ready")]
    SandboxReady,
    #[serde(rename = "text.delta")]
    TextDelta { delta: String },
    #[serde(rename = "reasoning.delta")]
    ReasoningDelta { delta: String },
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
    #[serde(rename = "run.completed")]
    RunCompleted,
    /// Waiting for answers to the session's `pending_questions`.
    #[serde(rename = "run.needs_input")]
    RunNeedsInput,
    #[serde(rename = "run.cancelled")]
    RunCancelled,
    #[serde(rename = "run.failed")]
    RunFailed { error: String },
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
        Event::TextDelta { delta } => SessionEvent::TextDelta { delta },
        Event::ReasoningDelta { delta } => SessionEvent::ReasoningDelta { delta },
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
    CurrentUser(user): CurrentUser,
    Path(id): Path<String>,
    Query(query): Query<EventsQuery>,
    headers: HeaderMap,
) -> Result<Sse<impl Stream<Item = Result<SseEvent, Infallible>>>, ApiError> {
    let session = find_session(&state, user, &id).await?;
    let resume = headers
        .get("last-event-id")
        .and_then(|value| value.to_str().ok())
        .map(str::to_owned);
    let mut after = match (resume, query.from.as_deref()) {
        (Some(id), _) => id,
        (None, Some("start")) => "0".to_owned(),
        (None, _) => state
            .store
            .last_event_id(session.id)
            .await?
            .unwrap_or_else(|| "0".to_owned()),
    };
    let mut connection = state.store.event_connection().await?;

    let (tx, rx) = mpsc::channel(64);
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

    let stream = stream::unfold(rx, |mut rx| async move {
        rx.recv().await.map(|event| (Ok(event), rx))
    });
    Ok(Sse::new(stream).keep_alive(KeepAlive::new().interval(KEEP_ALIVE)))
}
