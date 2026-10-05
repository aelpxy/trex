use std::collections::HashSet;

use anyhow::Context;
use async_openai::types::responses::{
    EasyInputContent, EasyInputMessage, InputItem, Item, MessageType, Role,
};
use serde_json::Value;

use crate::{agent::output_item, question::ASK_USER};

const UNANSWERED: &str = "The user did not answer and sent a new message instead.";
const INTERRUPTED: &str = "The tool call was interrupted before it finished.";

// marks the developer message that replaces everything before it in the model's context
const CHECKPOINT_HEADER: &str = "[trex context checkpoint]";
const CHECKPOINT_INTRO: &str = "The conversation above this point was compacted to save context. \
Below are the user's most recent messages, oldest first, then a summary of the work so far. \
Continue from where it left off; if a task was in progress, keep working on it without asking the user to repeat themselves.";
const TRANSCRIPT_RESULT_CHARS: usize = 2_000;
const OMITTED: &str = "[... earlier entries omitted ...]";

pub fn user_message(text: &str) -> InputItem {
    EasyInputMessage::from(text).into()
}

// a checkpoint is append-only like every other item; the model's context starts at the latest one
pub fn checkpoint(recent_user_messages: &[String], summary: &str) -> InputItem {
    let messages = recent_user_messages
        .iter()
        .map(|message| format!("<user_message>\n{message}\n</user_message>"))
        .collect::<Vec<_>>()
        .join("\n");
    let text = format!(
        "{CHECKPOINT_HEADER}\n{CHECKPOINT_INTRO}\n\n<recent_user_messages>\n{messages}\n</recent_user_messages>\n\n<summary>\n{summary}\n</summary>"
    );
    InputItem::EasyMessage(EasyInputMessage {
        r#type: MessageType::Message,
        role: Role::Developer,
        content: EasyInputContent::Text(text),
        phase: None,
    })
}

// users can only send user messages, so a developer message with the header is always ours
pub fn checkpoint_text(item: &Value) -> Option<&str> {
    if item["role"] != "developer" {
        return None;
    }
    item["content"].as_str()?.strip_prefix(CHECKPOINT_HEADER)
}

pub fn context_start(history: &[InputItem]) -> usize {
    history
        .iter()
        .rposition(|item| {
            serde_json::to_value(item).is_ok_and(|value| checkpoint_text(&value).is_some())
        })
        .unwrap_or(0)
}

// newest first until the budget is spent, returned oldest first
pub fn recent_user_messages(history: &[InputItem], budget_chars: usize) -> Vec<String> {
    let mut messages = Vec::new();
    let mut used = 0;
    for value in history
        .iter()
        .rev()
        .filter_map(|item| serde_json::to_value(item).ok())
    {
        if value["role"] != "user" {
            continue;
        }
        let text = message_text(&value);
        if text.is_empty() {
            continue;
        }
        used += text.len();
        if used > budget_chars {
            break;
        }
        messages.push(text);
    }
    messages.reverse();
    messages
}

// a plain-text rendering for summarizing a context too large to send as items; keeps the first
// entry (the task) and as many recent entries as fit
pub fn transcript(history: &[InputItem], budget_chars: usize) -> String {
    let entries: Vec<String> = history
        .iter()
        .filter_map(|item| serde_json::to_value(item).ok())
        .filter_map(|value| transcript_entry(&value))
        .collect();
    let Some((first, rest)) = entries.split_first() else {
        return String::new();
    };

    let mut used = first.len();
    let mut tail = Vec::new();
    for entry in rest.iter().rev() {
        if used + entry.len() > budget_chars {
            break;
        }
        used += entry.len();
        tail.push(entry.as_str());
    }
    tail.reverse();

    let mut parts = vec![truncate_chars(first, budget_chars)];
    if tail.len() < rest.len() {
        parts.push(OMITTED.to_owned());
    }
    parts.extend(tail.into_iter().map(str::to_owned));
    parts.join("\n\n")
}

fn transcript_entry(value: &Value) -> Option<String> {
    let kind = value["type"].as_str().unwrap_or("message");
    match kind {
        "message" => {
            let text = match checkpoint_text(value) {
                Some(text) => text.trim().to_owned(),
                None => message_text(value),
            };
            let role = value["role"].as_str().unwrap_or("user");
            (!text.is_empty()).then(|| format!("[{role}]\n{text}"))
        }
        "function_call" => Some(format!(
            "[tool call: {}]\n{}",
            value["name"].as_str().unwrap_or_default(),
            value["arguments"].as_str().unwrap_or_default()
        )),
        "function_call_output" => {
            let output = match &value["output"] {
                Value::String(text) => text.clone(),
                other => other.to_string(),
            };
            Some(format!(
                "[tool result]\n{}",
                truncate_chars(&output, TRANSCRIPT_RESULT_CHARS)
            ))
        }
        "reasoning" => {
            let summary = value["summary"]
                .as_array()?
                .iter()
                .filter_map(|part| part["text"].as_str())
                .collect::<Vec<_>>()
                .join("\n");
            (!summary.is_empty()).then(|| format!("[assistant reasoning]\n{summary}"))
        }
        _ => None,
    }
}

fn message_text(value: &Value) -> String {
    match &value["content"] {
        Value::String(text) => text.clone(),
        Value::Array(parts) => parts
            .iter()
            .filter_map(|part| part["text"].as_str())
            .collect::<Vec<_>>()
            .join(""),
        _ => String::new(),
    }
}

fn truncate_chars(text: &str, max: usize) -> String {
    match text.char_indices().nth(max) {
        Some((end, _)) => format!("{}...", &text[..end]),
        None => text.to_owned(),
    }
}

pub fn from_json(items: Vec<Value>) -> anyhow::Result<Vec<InputItem>> {
    items
        .into_iter()
        .map(|item| serde_json::from_value(item).context("invalid history item"))
        .collect()
}

pub fn to_json(items: &[InputItem]) -> anyhow::Result<Vec<Value>> {
    items
        .iter()
        .map(|item| serde_json::to_value(item).context("failed to serialize history item"))
        .collect()
}

// the responses api rejects a function call without an output, which happens when a user ignores a
// question or a run is interrupted mid-tool. outputs are appended rather than inserted after their
// call so history stays append-only; calls before the context start are never sent, so they're left
pub fn close_dangling_calls(history: &mut Vec<InputItem>) {
    let context = &history[context_start(history)..];
    let answered: HashSet<&str> = context
        .iter()
        .filter_map(|item| match item {
            InputItem::Item(Item::FunctionCallOutput(output)) => output.call_id.as_deref(),
            _ => None,
        })
        .collect();
    let closing: Vec<InputItem> = context
        .iter()
        .filter_map(|item| match item {
            InputItem::Item(Item::FunctionCall(call))
                if !answered.contains(call.call_id.as_str()) =>
            {
                let output = if call.name == ASK_USER {
                    UNANSWERED
                } else {
                    INTERRUPTED
                };
                Some(output_item(call.call_id.clone(), output.into()))
            }
            _ => None,
        })
        .collect();
    history.extend(closing);
}

// a run that ended with the model's reply has nothing left to do, which matters when one is resumed
pub fn ends_with_reply(history: &[InputItem]) -> bool {
    history.last().is_some_and(|item| {
        let value = serde_json::to_value(item).unwrap_or_default();
        value["type"] == "message" && value["role"] == "assistant"
    })
}

#[cfg(test)]
mod tests {
    use async_openai::types::responses::FunctionToolCall;
    use serde_json::json;

    use super::*;

    fn call(id: &str, name: &str) -> InputItem {
        let call: FunctionToolCall = serde_json::from_value(
            json!({"type": "function_call", "arguments": "{}", "call_id": id, "name": name}),
        )
        .unwrap();
        InputItem::Item(Item::FunctionCall(call))
    }

    fn output_of(item: &InputItem) -> (String, String) {
        let value = serde_json::to_value(item).unwrap();
        (
            value["call_id"].as_str().unwrap().to_owned(),
            value["output"].as_str().unwrap().to_owned(),
        )
    }

    #[test]
    fn closes_dangling_calls_at_the_end() {
        let mut history = vec![
            call("q1", ASK_USER),
            output_item("q1".into(), "answered".into()),
            call("q2", ASK_USER),
            call("b1", "bash"),
            user_message("never mind"),
        ];
        close_dangling_calls(&mut history);

        assert_eq!(history.len(), 7);
        assert_eq!(output_of(&history[5]), ("q2".into(), UNANSWERED.into()));
        assert_eq!(output_of(&history[6]), ("b1".into(), INTERRUPTED.into()));
        close_dangling_calls(&mut history);
        assert_eq!(history.len(), 7, "closing is idempotent");
    }

    #[test]
    fn leaves_calls_before_the_context_alone() {
        let mut history = vec![
            call("old", "bash"),
            checkpoint(&[], "summary"),
            call("new", "bash"),
        ];
        close_dangling_calls(&mut history);

        assert_eq!(history.len(), 4);
        assert_eq!(output_of(&history[3]), ("new".into(), INTERRUPTED.into()));
    }

    #[test]
    fn detects_a_finished_reply() {
        assert!(ends_with_reply(&[user_message("hi"), assistant("hello")]));
        assert!(!ends_with_reply(&[assistant("hello"), user_message("hi")]));
        assert!(!ends_with_reply(&[]));
    }

    fn assistant(text: &str) -> InputItem {
        serde_json::from_value(json!({"type": "message", "id": "msg_1", "status": "completed", "role": "assistant", "content": [{"type": "output_text", "text": text, "annotations": []}]})).unwrap()
    }

    #[test]
    fn context_starts_at_the_latest_checkpoint() {
        let mut history = vec![user_message("first"), assistant("one")];
        assert_eq!(context_start(&history), 0);

        history.push(checkpoint(&["first".into()], "did one"));
        history.push(user_message("second"));
        history.push(checkpoint(&["first".into(), "second".into()], "did two"));
        history.push(user_message("third"));
        assert_eq!(context_start(&history), 4);

        let value = to_json(&history[4..5]).unwrap().remove(0);
        let text = checkpoint_text(&value).unwrap();
        assert!(text.contains("<user_message>\nsecond\n</user_message>"));
        assert!(text.contains("<summary>\ndid two\n</summary>"));
        assert!(checkpoint_text(&json!({"role": "user", "content": CHECKPOINT_HEADER})).is_none());
    }

    #[test]
    fn keeps_the_newest_user_messages_within_budget() {
        let history = vec![
            user_message("aaaa"),
            assistant("ignored"),
            user_message("bbbb"),
            checkpoint(&[], "not a user message"),
            user_message("cccc"),
        ];
        assert_eq!(recent_user_messages(&history, 8), ["bbbb", "cccc"]);
        assert_eq!(
            recent_user_messages(&history, 100),
            ["aaaa", "bbbb", "cccc"]
        );
    }

    #[test]
    fn transcript_keeps_the_task_and_the_latest_entries() {
        let long_output = "x".repeat(5_000);
        let history = vec![
            user_message("build the thing"),
            call("c1", "bash"),
            output_item("c1".into(), long_output),
            assistant("middle"),
            assistant("latest"),
        ];

        let full = transcript(&history, 100_000);
        assert!(full.starts_with("[user]\nbuild the thing"));
        assert!(full.contains("[tool call: bash]\n{}"));
        assert!(full.contains(&format!("{}...", "x".repeat(TRANSCRIPT_RESULT_CHARS))));
        assert!(!full.contains(OMITTED));

        let short = transcript(&history, 60);
        assert_eq!(
            short,
            format!(
                "[user]\nbuild the thing\n\n{OMITTED}\n\n[assistant]\nmiddle\n\n[assistant]\nlatest"
            )
        );
    }

    #[test]
    fn round_trips_history_through_json() {
        let history = vec![
            user_message("hi"),
            call("c1", "bash"),
            output_item("c1".into(), "ok".into()),
        ];
        let json = to_json(&history).unwrap();
        let restored = from_json(json.clone()).unwrap();
        assert_eq!(to_json(&restored).unwrap(), json);
    }
}
