use std::collections::HashSet;

use anyhow::Context;
use async_openai::types::responses::{EasyInputMessage, InputItem, Item};
use serde_json::Value;

use crate::{agent::output_item, question::ASK_USER};

const UNANSWERED: &str = "The user did not answer and sent a new message instead.";
const INTERRUPTED: &str = "The tool call was interrupted before it finished.";

pub fn user_message(text: &str) -> InputItem {
    EasyInputMessage::from(text).into()
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
// question or a run is cancelled mid-tool, so every dangling call is closed before the next request
pub fn close_dangling_calls(history: &mut Vec<InputItem>) {
    let answered: HashSet<String> = history
        .iter()
        .filter_map(|item| match item {
            InputItem::Item(Item::FunctionCallOutput(output)) => output.call_id.clone(),
            _ => None,
        })
        .collect();
    let dangling: Vec<(String, bool)> = history
        .iter()
        .filter_map(|item| match item {
            InputItem::Item(Item::FunctionCall(call)) if !answered.contains(&call.call_id) => {
                Some((call.call_id.clone(), call.name == ASK_USER))
            }
            _ => None,
        })
        .collect();

    for (call_id, is_question) in dangling {
        let position = history
            .iter()
            .position(|item| matches!(item, InputItem::Item(Item::FunctionCall(c)) if c.call_id == call_id))
            .expect("call was just found in history");
        let output = if is_question { UNANSWERED } else { INTERRUPTED };
        history.insert(position + 1, output_item(call_id, output.into()));
    }
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
    fn closes_dangling_calls_after_each_call() {
        let mut history = vec![
            call("q1", ASK_USER),
            output_item("q1".into(), "answered".into()),
            call("q2", ASK_USER),
            call("b1", "bash"),
            user_message("never mind"),
        ];
        close_dangling_calls(&mut history);

        assert_eq!(history.len(), 7);
        assert_eq!(output_of(&history[3]), ("q2".into(), UNANSWERED.into()));
        assert_eq!(output_of(&history[5]), ("b1".into(), INTERRUPTED.into()));
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
