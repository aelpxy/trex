mod bash;
mod file;

use anyhow::Context;
use async_openai::types::responses::{FunctionTool, Tool as ToolDefinition};
use futures::future::BoxFuture;
use serde_json::Value;
use tokio::sync::mpsc;
use trex_sandbox::{OpenShell, Sandbox};

pub use self::{
    bash::Bash,
    file::{EditFile, ReadFile, WriteFile},
};
use crate::event::Event;

// keeps a single noisy result from flooding the model context
const MAX_OUTPUT_BYTES: usize = 32 * 1024;

pub struct ToolContext<'a> {
    pub openshell: &'a OpenShell,
    pub sandbox: &'a Sandbox,
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

// a vec keeps tool order stable across requests so the prompt prefix stays cacheable
pub struct Tools {
    tools: Vec<Box<dyn Tool>>,
}

impl Tools {
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        Self { tools }
    }

    pub fn standard() -> Self {
        Self::new(vec![
            Box::new(Bash),
            Box::new(ReadFile),
            Box::new(WriteFile),
            Box::new(EditFile),
        ])
    }

    pub fn definitions(&self) -> Vec<ToolDefinition> {
        self.tools
            .iter()
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
            .iter()
            .find(|tool| tool.definition().name == name)
            .with_context(|| format!("unknown tool {name}"))?;
        let args = serde_json::from_str(arguments).context("tool arguments are not valid json")?;
        tool.call(ctx, args).await
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
