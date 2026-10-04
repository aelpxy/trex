use std::{collections::HashSet, time::Duration};

use anyhow::{Context, bail};
use async_openai::types::responses::{
    FunctionCallOutput, FunctionCallOutputItemParam, FunctionToolCall, InputItem, Item,
    MessageItem, OutputItem, ReasoningEffort, Response, ResponseStreamEvent,
};
use futures::{StreamExt, future::join_all};
use tokio::{sync::mpsc, time::sleep};
use trex_sandbox::{OpenShell, Sandbox};
use trex_store::library::Library;
use uuid::Uuid;

use crate::{
    event::{Event, Usage},
    model::{Model, Turn},
    tool::{ToolContext, Tools},
};

const ACCESS_POLL_INTERVAL: Duration = Duration::from_secs(5);

pub struct Agent<'a> {
    pub user: Uuid,
    pub library: &'a Library,
    pub model: &'a Model,
    pub tools: &'a Tools,
    pub openshell: &'a OpenShell,
    pub sandbox: &'a Sandbox,
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
        tokio::select! {
            result = self.run_turns(history, events) => result,
            result = self.watch_access(events) => result,
        }
    }

    async fn run_turns(
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

    // surfaces each network request the sandbox was denied once, so the user can approve it mid-run
    async fn watch_access(&self, events: &mpsc::Sender<Event>) -> anyhow::Result<()> {
        let mut seen = HashSet::new();
        loop {
            sleep(ACCESS_POLL_INTERVAL).await;
            let requests = match self.openshell.pending_access(self.sandbox).await {
                Ok(requests) => requests,
                Err(error) => {
                    tracing::warn!(
                        error = format!("{error:#}"),
                        "failed to poll access requests"
                    );
                    continue;
                }
            };
            for request in requests {
                if seen.insert(request.id.clone()) {
                    send(events, Event::AccessRequest(request)).await?;
                }
            }
        }
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
            user: self.user,
            library: self.library,
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
    use async_openai::types::responses::EasyInputMessage;

    use super::*;
    use crate::{
        model::{TEST_MODEL, test_models},
        test_support::sandbox_for_new_user,
        tool::Tools,
    };

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn runs_bash_in_sandbox() {
        let models = test_models();
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let tools = Tools::standard();

        let library = Library::in_memory();
        let agent = Agent {
            user,
            library: &library,
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
        openshell.delete_workspace(user).await.unwrap();

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
                .all(|u| u.model == TEST_MODEL && u.input_tokens > 0)
        );
        assert_eq!(text.trim(), "trex");
        assert!(
            history
                .iter()
                .any(|item| matches!(item, InputItem::Item(Item::FunctionCallOutput(_))))
        );
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn replays_encrypted_reasoning() {
        let models = test_models();
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let tools = Tools::standard();

        let library = Library::in_memory();
        let agent = Agent {
            user,
            library: &library,
            model: models.get(TEST_MODEL).unwrap(),
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: None,
            reasoning_effort: Some(ReasoningEffort::High),
            max_turns: 10,
        };
        let mut history = vec![
            EasyInputMessage::from(
                "How many primes are below 60? Work it out, verify it with the bash tool, then reply with only the number.",
            )
            .into(),
        ];
        let (tx, mut rx) = mpsc::channel(256);
        let collector = tokio::spawn(async move {
            let mut text = String::new();
            while let Some(event) = rx.recv().await {
                if let Event::TextDelta { delta } = event {
                    text.push_str(&delta);
                }
            }
            text
        });

        let result = agent.run(&mut history, &tx).await;
        drop(tx);
        let text = collector.await.unwrap();
        openshell.delete_workspace(user).await.unwrap();

        result.unwrap();
        assert_eq!(text.trim(), "17");
        // the reasoning precedes a tool call, so the follow-up turn resent it and was accepted
        let reasoning = history.iter().position(|item| {
            matches!(item, InputItem::Item(Item::Reasoning(r)) if r.encrypted_content.is_some())
        });
        let call = history
            .iter()
            .position(|item| matches!(item, InputItem::Item(Item::FunctionCall(_))));
        assert!(matches!((reasoning, call), (Some(r), Some(c)) if r < c));
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn surfaces_denied_network_access() {
        let models = test_models();
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let tools = Tools::standard();
        let library = Library::in_memory();
        let agent = Agent {
            user,
            library: &library,
            model: models.get(TEST_MODEL).unwrap(),
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: None,
            reasoning_effort: None,
            max_turns: 1,
        };

        let script = "timeout 15 bash -c 'exec 3<>/dev/tcp/example.com/443'";
        let denied = openshell
            .output(
                &sandbox,
                ["bash", "-c", script].map(String::from).to_vec(),
                Vec::new(),
            )
            .await
            .unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let first = tokio::time::timeout(Duration::from_secs(90), async {
            tokio::select! {
                result = agent.watch_access(&tx) => panic!("watcher stopped: {result:?}"),
                event = rx.recv() => event,
            }
        })
        .await;

        openshell.delete_workspace(user).await.unwrap();

        assert_ne!(denied.exit_code, Some(0));
        let Ok(Some(Event::AccessRequest(request))) = first else {
            panic!("expected an access request event");
        };
        assert!(request.endpoints.iter().any(|e| e == "example.com:443"));
        assert!(!request.review_token.is_empty());
    }
}
