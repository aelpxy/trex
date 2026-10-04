use std::{convert::Infallible, sync::Arc, time::Duration};

use axum::{
    extract::{Path, Query, State},
    http::HeaderMap,
    response::sse::{Event as SseEvent, KeepAlive, Sse},
};
use futures::{Stream, stream};
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use trex_harness::event::{Event, OutputStream};

use super::{AppState, auth::CurrentUser, error::ApiError, sessions::find_session};

const READ_BLOCK: Duration = Duration::from_secs(15);
const KEEP_ALIVE: Duration = Duration::from_secs(15);

#[derive(Deserialize)]
pub struct EventsQuery {
    from: Option<String>,
}

// harness events become api events here; anything internal (review tokens, raw items) stays out
pub fn to_api(event: &Event) -> Option<Value> {
    let value = match event {
        Event::TextDelta { delta } => json!({"type": "text.delta", "delta": delta}),
        Event::ReasoningDelta { delta } => json!({"type": "reasoning.delta", "delta": delta}),
        Event::ToolCall {
            call_id,
            name,
            arguments,
        } => json!({"type": "tool.call", "call_id": call_id, "name": name, "arguments": arguments}),
        Event::ToolOutput {
            call_id,
            stream,
            chunk,
        } => {
            let stream = match stream {
                OutputStream::Stdout => "stdout",
                OutputStream::Stderr => "stderr",
            };
            json!({"type": "tool.output", "call_id": call_id, "stream": stream, "chunk": chunk})
        }
        Event::ToolResult {
            call_id,
            output,
            is_error,
        } => {
            json!({"type": "tool.result", "call_id": call_id, "output": output, "is_error": is_error})
        }
        Event::Usage(usage) => json!({
            "type": "usage",
            "model": usage.model,
            "input_tokens": usage.input_tokens,
            "cached_input_tokens": usage.cached_input_tokens,
            "cache_write_tokens": usage.cache_write_tokens,
            "output_tokens": usage.output_tokens,
            "reasoning_tokens": usage.reasoning_tokens,
        }),
        Event::AccessRequest(request) => json!({
            "type": "access.requested",
            "id": request.id,
            "endpoints": request.endpoints,
            "binary": request.binary,
            "rationale": request.rationale,
            "security_notes": request.security_notes,
        }),
        Event::Question { questions, .. } => json!({"type": "question", "questions": questions}),
        Event::Done => return None,
    };
    Some(value)
}

// without Last-Event-ID the stream starts at the live tail, or at the beginning with ?from=start
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
