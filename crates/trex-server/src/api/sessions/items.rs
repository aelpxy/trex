use std::sync::Arc;

use axum::{
    Json,
    extract::{Path, State},
};
use serde::Serialize;
use serde_json::Value;
use trex_harness::{attachment, history};
use utoipa::ToSchema;

use super::{ATTACHMENT_PREFIX, find_session};
use crate::api::{
    AppState, List,
    auth::Auth,
    error::{ApiError, ErrorResponse},
};

/// A file attached to a message; download it from `GET /v1/attachments/{id}`.
#[derive(Serialize, ToSchema)]
pub struct Attachment {
    #[schema(example = "att_9f86d081884c7d659a2feaa0c55ad015a3bf4f1b2b0b822cd15d6c15b0f00a08")]
    id: String,
    #[schema(example = "image")]
    kind: &'static str,
    #[schema(example = "image/png")]
    mime_type: String,
    filename: Option<String>,
}

/// A conversation item, rendered by `type`.
#[derive(Serialize, ToSchema)]
#[serde(tag = "type", rename_all = "snake_case")]
pub enum Item {
    Message {
        #[schema(example = "assistant")]
        role: String,
        text: String,
        /// Files the user attached; empty for other messages.
        attachments: Vec<Attachment>,
    },
    ToolCall {
        call_id: String,
        name: String,
        /// JSON-encoded arguments.
        arguments: String,
    },
    ToolResult {
        call_id: String,
        output: String,
    },
    Reasoning {
        summary: String,
    },
    /// Everything before this item was summarized to free up context; the model sees only this
    /// summary and the items after it.
    Compaction {
        summary: String,
    },
}

/// An item with when it was saved.
#[derive(Serialize, ToSchema)]
pub struct TimedItem {
    #[serde(flatten)]
    item: Item,
    /// The item's position in the conversation, counting from 1; see `run.started`'s `items`.
    seq: i64,
    /// Unix milliseconds.
    created_at: i64,
}

/// One model response.
#[derive(Serialize, ToSchema)]
pub struct UsageEntry {
    /// Unix milliseconds, when the response finished.
    created_at: i64,
    model: String,
    input_tokens: i64,
    cached_input_tokens: i64,
    cache_write_tokens: i64,
    output_tokens: i64,
    reasoning_tokens: i64,
    credits: i64,
    /// From sending the request to the end of the response.
    duration_ms: i64,
    /// Until the first streamed output; null for older records or when nothing streamed.
    first_token_ms: Option<i64>,
}

/// List conversation items
///
/// The whole conversation, oldest first.
#[utoipa::path(
    get,
    operation_id = "list_items",
    path = "/sessions/{id}/items",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 200, body = List<TimedItem>),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn items(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<List<TimedItem>>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let items = state
        .store
        .session_items_timed(workspace, session.id)
        .await?;
    let data = items
        .iter()
        .filter_map(|(value, seq, created_at)| {
            item(value).map(|item| TimedItem {
                item,
                seq: *seq,
                created_at: *created_at,
            })
        })
        .collect();
    Ok(Json(List::new(data, false)))
}

/// List usage
///
/// Every model response of the session with its tokens, credits and duration, oldest first. Match
/// them to turns by `created_at` against the items'.
#[utoipa::path(
    get,
    operation_id = "list_session_usage",
    path = "/sessions/{id}/usage",
    tag = "sessions",
    params(("id" = String, Path, description = "Session id")),
    responses(
        (status = 200, body = List<UsageEntry>),
        (status = 404, response = ErrorResponse),
    ),
)]
pub async fn usage(
    State(state): State<Arc<AppState>>,
    Auth { workspace, .. }: Auth,
    Path(id): Path<String>,
) -> Result<Json<List<UsageEntry>>, ApiError> {
    let session = find_session(&state, workspace, &id).await?;
    let entries = state.store.session_usage(workspace, session.id).await?;
    let data = entries
        .into_iter()
        .map(|entry| UsageEntry {
            created_at: entry.created_at_ms,
            model: entry.model,
            input_tokens: entry.input_tokens,
            cached_input_tokens: entry.cached_input_tokens,
            output_tokens: entry.output_tokens,
            reasoning_tokens: entry.reasoning_tokens,
            credits: entry.credits,
            duration_ms: entry.duration_ms,
            first_token_ms: entry.first_token_ms,
            cache_write_tokens: entry.cache_write_tokens,
        })
        .collect();
    Ok(Json(List::new(data, false)))
}

pub(super) fn attachments_of(item: &Value) -> Vec<Attachment> {
    attachment::references(item)
        .into_iter()
        .map(|(kind, hash, mime_type, filename)| Attachment {
            id: format!("{ATTACHMENT_PREFIX}{hash}"),
            kind,
            mime_type,
            filename,
        })
        .collect()
}

// converts stored responses api items into trex's own item shapes for the ui
fn item(item: &Value) -> Option<Item> {
    let kind = item["type"].as_str().unwrap_or("message");
    match kind {
        "message" if history::checkpoint_text(item).is_some() => Some(Item::Compaction {
            summary: history::checkpoint_text(item)?.trim().to_owned(),
        }),
        // developer messages are the harness talking to the model, not part of the conversation
        "message" if item["role"] == "developer" => None,
        "message" => {
            if !matches!(item["content"], Value::String(_) | Value::Array(_)) {
                return None;
            }
            let text = history::message_text(item);
            let attachments = attachments_of(item);
            Some(Item::Message {
                role: text_of(&item["role"]),
                text,
                attachments,
            })
        }
        "function_call" => Some(Item::ToolCall {
            call_id: text_of(&item["call_id"]),
            name: text_of(&item["name"]),
            arguments: text_of(&item["arguments"]),
        }),
        "function_call_output" => Some(Item::ToolResult {
            call_id: text_of(&item["call_id"]),
            output: output_text(&item["output"]),
        }),
        "reasoning" => {
            let summary: Vec<&str> = item["summary"]
                .as_array()?
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect();
            (!summary.is_empty()).then(|| Item::Reasoning {
                summary: summary.join("\n\n"),
            })
        }
        _ => None,
    }
}

// tool output saved as content parts reads as its text, each image as a reference the ui can show
fn output_text(value: &Value) -> String {
    let Value::Array(parts) = value else {
        return text_of(value);
    };
    parts
        .iter()
        .map(|part| match part["type"].as_str() {
            Some("input_text") => text_of(&part["text"]),
            Some("input_image") => {
                attachment::image_placeholder(part["image_url"].as_str().unwrap_or_default())
            }
            _ => "[file]".to_owned(),
        })
        .collect::<Vec<_>>()
        .join("\n")
}

fn text_of(value: &Value) -> String {
    match value {
        Value::String(text) => text.clone(),
        other => other.to_string(),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    use serde_json::json;

    fn item_json(value: &Value) -> Option<Value> {
        item(value).map(|item| serde_json::to_value(item).unwrap())
    }

    #[test]
    fn maps_stored_items_to_api_items() {
        let user = json!({"role": "user", "content": "hi"});
        let assistant = json!({"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "hello"}]});
        let call =
            json!({"type": "function_call", "call_id": "c1", "name": "bash", "arguments": "{}"});
        let output = json!({"type": "function_call_output", "call_id": "c1", "output": "ok"});
        let reasoning = json!({"type": "reasoning", "summary": [{"type": "summary_text", "text": "thinking"}], "encrypted_content": "secret"});
        let hidden = json!({"type": "reasoning", "summary": [], "encrypted_content": "secret"});

        assert_eq!(item_json(&user).unwrap()["text"], "hi");
        assert_eq!(item_json(&assistant).unwrap()["text"], "hello");
        assert_eq!(item_json(&call).unwrap()["type"], "tool_call");
        assert_eq!(item_json(&output).unwrap()["output"], "ok");
        let shown = json!({"type": "function_call_output", "call_id": "c2", "output": [
            {"type": "input_text", "text": "the page:"},
            {"type": "input_image", "image_url": "attachment://abc#image/jpeg", "detail": "auto"}
        ]});
        assert_eq!(
            item_json(&shown).unwrap()["output"],
            "the page:\n[image: attachment://abc#image/jpeg]"
        );
        assert_eq!(
            item_json(&reasoning).unwrap(),
            json!({"type": "reasoning", "summary": "thinking"})
        );
        assert!(item_json(&hidden).is_none());

        let checkpoint =
            serde_json::to_value(history::checkpoint(&["hi".into()], "did things")).unwrap();
        let compaction = item_json(&checkpoint).unwrap();
        assert_eq!(compaction["type"], "compaction");
        assert!(
            compaction["summary"]
                .as_str()
                .unwrap()
                .ends_with("<summary>\ndid things\n</summary>")
        );
    }
}
