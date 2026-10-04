use std::{collections::HashMap, time::Duration};

use anyhow::{Context, bail};
use async_openai::types::responses::{FunctionTool, Tool as ToolDefinition};
use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::{sync::mpsc, time::timeout};
use trex_sandbox::{ExecEvent, OpenShell};

use crate::event::{Event, OutputStream};

const BASH_TIMEOUT: Duration = Duration::from_secs(120);
// keeps a single noisy command from flooding the model context
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

pub struct ToolContext<'a> {
    pub openshell: &'a OpenShell,
    pub sandbox: &'a str,
    pub call_id: &'a str,
    pub events: &'a mpsc::Sender<Event>,
}

pub trait Tool: Send + Sync {
    fn definition(&self) -> FunctionTool;

    fn call<'a>(
        &'a self,
        ctx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, anyhow::Result<String>>;
}

pub struct Tools {
    tools: HashMap<String, Box<dyn Tool>>,
}

impl Tools {
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        let tools = tools
            .into_iter()
            .map(|tool| (tool.definition().name, tool))
            .collect();
        Self { tools }
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .values()
            .map(|tool| ToolDefinition::Function(tool.definition()))
            .collect()
    }

    pub async fn call(
        &self,
        ctx: ToolContext<'_>,
        name: &str,
        arguments: &str,
    ) -> anyhow::Result<String> {
        let tool = self
            .tools
            .get(name)
            .with_context(|| format!("unknown tool {name}"))?;
        let args = serde_json::from_str(arguments).context("tool arguments are not valid json")?;
        tool.call(ctx, args).await
    }
}

pub struct Bash;

impl Tool for Bash {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "bash".into(),
            description: Some(
                "Run a bash command in the sandbox and return its stdout, stderr and exit code."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The bash command to run."}
                },
                "required": ["command"],
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
            let Some(command) = args["command"].as_str() else {
                bail!("missing command");
            };

            let argv = vec!["bash".to_owned(), "-c".to_owned(), command.to_owned()];
            let mut stream = ctx.openshell.exec(ctx.sandbox, argv, None).await?;

            let mut output = Vec::new();
            let mut exit_code = None;
            let finished = timeout(BASH_TIMEOUT, async {
                while let Some(event) = stream.next().await? {
                    let (stream, data) = match event {
                        ExecEvent::Stdout(data) => (OutputStream::Stdout, data),
                        ExecEvent::Stderr(data) => (OutputStream::Stderr, data),
                        ExecEvent::Exit(code) => {
                            exit_code = Some(code);
                            continue;
                        }
                    };
                    let chunk = String::from_utf8_lossy(&data).into_owned();
                    output.extend_from_slice(&data);
                    let event = Event::ToolOutput {
                        call_id: ctx.call_id.to_owned(),
                        stream,
                        chunk,
                    };
                    ctx.events
                        .send(event)
                        .await
                        .context("event receiver dropped")?;
                }
                anyhow::Ok(())
            })
            .await;

            let mut result = truncate(&String::from_utf8_lossy(&output));
            match (finished, exit_code) {
                (Err(_), _) => {
                    result.push_str(&format!("\n[timed out after {}s]", BASH_TIMEOUT.as_secs()))
                }
                (Ok(Err(error)), _) => return Err(error),
                (Ok(Ok(())), Some(code)) => result.push_str(&format!("\n[exit code {code}]")),
                (Ok(Ok(())), None) => result.push_str("\n[exited without a status]"),
            }
            Ok(result)
        })
    }
}

fn truncate(output: &str) -> String {
    if output.len() <= MAX_OUTPUT_BYTES {
        return output.to_owned();
    }
    let half = MAX_OUTPUT_BYTES / 2;
    let head = output.floor_char_boundary(half);
    let tail = output.ceil_char_boundary(output.len() - half);
    format!(
        "{}\n[... {} bytes truncated ...]\n{}",
        &output[..head],
        tail - head,
        &output[tail..]
    )
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn truncate_keeps_short_output() {
        assert_eq!(truncate("hello"), "hello");
    }

    #[test]
    fn truncate_keeps_head_and_tail() {
        let output = format!(
            "{}{}",
            "a".repeat(MAX_OUTPUT_BYTES),
            "b".repeat(MAX_OUTPUT_BYTES)
        );
        let truncated = truncate(&output);
        assert!(truncated.starts_with(&"a".repeat(MAX_OUTPUT_BYTES / 2)));
        assert!(truncated.ends_with(&"b".repeat(MAX_OUTPUT_BYTES / 2)));
        assert!(truncated.contains(&format!("[... {MAX_OUTPUT_BYTES} bytes truncated ...]")));
    }
}
