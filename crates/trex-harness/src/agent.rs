use anyhow::{Context, bail};
use async_openai::types::responses::{
    FunctionCallOutput, FunctionCallOutputItemParam, FunctionToolCall, InputItem, Item,
    MessageItem, OutputItem, ReasoningEffort, Response, ResponseStreamEvent,
};
use futures::{StreamExt, future::join_all};
use tokio::sync::mpsc;
use trex_sandbox::OpenShell;

use crate::{
    event::{Event, Usage},
    model::{Model, Turn},
    tool::{ToolContext, Tools},
};

pub struct Agent<'a> {
    pub model: &'a Model,
    pub tools: &'a Tools,
    pub openshell: &'a OpenShell,
    pub sandbox: &'a str,
    pub instructions: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub max_turns: usize,
}

impl Agent<'_> {
    // appends every item the run produces to history so the next run continues from it
    pub async fn run(
        &self,
        history: &mut Vec<InputItem>,
        events: &mpsc::Sender<Event>,
    ) -> anyhow::Result<()> {
        for turn in 0..self.max_turns {
            tracing::debug!(turn, model = self.model.id(), "starting turn");

            let calls = self.stream_turn(history, events).await?;
            if calls.is_empty() {
                send(events, Event::Done).await?;
                return Ok(());
            }

            let results = join_all(calls.iter().map(|call| self.call_tool(call, events))).await;
            for (call, output) in calls.into_iter().zip(results) {
                history.push(InputItem::Item(Item::FunctionCallOutput(
                    FunctionCallOutputItemParam {
                        call_id: Some(call.call_id),
                        output: FunctionCallOutput::Text(output?),
                        id: None,
                        status: None,
                        caller: None,
                        name: None,
                        namespace: None,
                    },
                )));
            }
        }

        bail!(
            "run stopped after reaching the limit of {} turns",
            self.max_turns
        )
    }

    async fn stream_turn(
        &self,
        history: &mut Vec<InputItem>,
        events: &mpsc::Sender<Event>,
    ) -> anyhow::Result<Vec<FunctionToolCall>> {
        let turn = Turn {
            instructions: self.instructions.clone(),
            input: history.clone(),
            tools: self.tools.definitions(),
            reasoning_effort: self.reasoning_effort.clone(),
        };
        let mut stream = self.model.stream(turn).await?;

        let mut calls = Vec::new();
        while let Some(event) = stream.next().await {
            match event.context("model stream failed")? {
                ResponseStreamEvent::ResponseOutputTextDelta(e) => {
                    send(events, Event::TextDelta { delta: e.delta }).await?;
                }
                ResponseStreamEvent::ResponseReasoningSummaryTextDelta(e) => {
                    send(events, Event::ReasoningDelta { delta: e.delta }).await?;
                }
                ResponseStreamEvent::ResponseOutputItemDone(e) => match e.item {
                    OutputItem::Message(message) => {
                        history.push(Item::Message(MessageItem::Output(message)).into());
                    }
                    OutputItem::Reasoning(reasoning) => {
                        history.push(Item::Reasoning(reasoning).into());
                    }
                    OutputItem::FunctionCall(call) => {
                        send(
                            events,
                            Event::ToolCall {
                                call_id: call.call_id.clone(),
                                name: call.name.clone(),
                                arguments: call.arguments.clone(),
                            },
                        )
                        .await?;
                        history.push(Item::FunctionCall(call.clone()).into());
                        calls.push(call);
                    }
                    other => tracing::warn!(item = ?other, "ignoring unsupported output item"),
                },
                ResponseStreamEvent::ResponseCompleted(e) => {
                    self.report_usage(&e.response, events).await?;
                    return Ok(calls);
                }
                ResponseStreamEvent::ResponseIncomplete(e) => {
                    self.report_usage(&e.response, events).await?;
                    let reason = e.response.incomplete_details.map(|d| d.reason);
                    bail!("model response incomplete: {}", reason.unwrap_or_default());
                }
                ResponseStreamEvent::ResponseFailed(e) => {
                    self.report_usage(&e.response, events).await?;
                    let message = e.response.error.map(|e| e.message);
                    bail!("model response failed: {}", message.unwrap_or_default());
                }
                ResponseStreamEvent::ResponseError(e) => bail!("model error: {}", e.message),
                _ => {}
            }
        }

        bail!("model stream ended before the response completed")
    }

    // incomplete and failed responses still consume tokens, so every terminal response is metered
    async fn report_usage(
        &self,
        response: &Response,
        events: &mpsc::Sender<Event>,
    ) -> anyhow::Result<()> {
        let Some(usage) = &response.usage else {
            tracing::warn!(model = self.model.id(), "response has no usage");
            return Ok(());
        };
        let usage = Usage {
            model: self.model.id().to_owned(),
            input_tokens: usage.input_tokens.into(),
            cached_input_tokens: usage.input_tokens_details.cached_tokens.into(),
            cache_write_tokens: usage
                .input_tokens_details
                .cache_write_tokens
                .map_or(0, |tokens| tokens.max(0) as u64),
            output_tokens: usage.output_tokens.into(),
            reasoning_tokens: usage.output_tokens_details.reasoning_tokens.into(),
        };
        send(events, Event::Usage(usage)).await
    }

    // tool failures are reported to the model as output so it can recover
    async fn call_tool(
        &self,
        call: &FunctionToolCall,
        events: &mpsc::Sender<Event>,
    ) -> anyhow::Result<String> {
        let ctx = ToolContext {
            openshell: self.openshell,
            sandbox: self.sandbox,
            call_id: &call.call_id,
            events,
        };

        let (output, is_error) = match self.tools.call(ctx, &call.name, &call.arguments).await {
            Ok(output) => (output, false),
            Err(error) => (format!("error: {error:#}"), true),
        };
        tracing::debug!(
            call_id = call.call_id,
            name = call.name,
            is_error,
            "tool finished"
        );

        send(
            events,
            Event::ToolResult {
                call_id: call.call_id.clone(),
                output: output.clone(),
                is_error,
            },
        )
        .await?;
        Ok(output)
    }
}

async fn send(events: &mpsc::Sender<Event>, event: Event) -> anyhow::Result<()> {
    events.send(event).await.context("event receiver dropped")
}

#[cfg(test)]
mod tests {
    use std::path::Path;

    use async_openai::types::responses::EasyInputMessage;

    use super::*;
    use crate::{
        model::{TEST_MODEL, test_models},
        tool::Tools,
    };

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn runs_bash_in_sandbox() {
        let models = test_models();
        let tls_dir = Path::new(env!("CARGO_MANIFEST_DIR")).join("../../certs/openshell");
        let openshell = OpenShell::connect("https://127.0.0.1:17670", &tls_dir)
            .await
            .unwrap();
        let sandbox = openshell.create(None).await.unwrap();
        let tools = Tools::standard();

        let agent = Agent {
            model: models.get(TEST_MODEL).unwrap(),
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: Some("You are a coding agent. Use the bash tool to act.".into()),
            reasoning_effort: Some(ReasoningEffort::Low),
            max_turns: 10,
        };

        let mut history = vec![
            EasyInputMessage::from(
                "Write the word trex into /tmp/name.txt, then read the file back and reply with only its contents.",
            )
            .into(),
        ];
        let (tx, mut rx) = mpsc::channel(256);
        let collector = tokio::spawn(async move {
            let (mut text, mut tool_results, mut done) = (String::new(), 0, false);
            let mut usages = Vec::new();
            while let Some(event) = rx.recv().await {
                match event {
                    Event::TextDelta { delta } => text.push_str(&delta),
                    Event::ToolResult { .. } => tool_results += 1,
                    Event::Usage(usage) => usages.push(usage),
                    Event::Done => done = true,
                    _ => {}
                }
            }
            (text, tool_results, done, usages)
        });

        let result = agent.run(&mut history, &tx).await;
        drop(tx);
        let (text, tool_results, done, usages) = collector.await.unwrap();
        openshell.delete(&sandbox).await.unwrap();

        result.unwrap();
        assert!(done);
        assert!(tool_results >= 1);
        assert!(
            usages.len() >= 2,
            "a tool turn and a final turn are both metered"
        );
        assert!(
            usages
                .iter()
                .all(|u| u.model == "gpt-6.1-sol" && u.input_tokens > 0)
        );
        assert_eq!(text.trim(), "trex");
        assert!(
            history
                .iter()
                .any(|item| matches!(item, InputItem::Item(Item::FunctionCallOutput(_))))
        );
    }
}
