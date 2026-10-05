mod bash;
mod file;
mod image;
mod library;
mod patch;
mod plan;
mod process;
mod search;
mod time;
mod web;

use anyhow::Context;
use async_openai::types::responses::{
    FunctionCallOutput, FunctionTool, InputContent, Tool as ToolDefinition,
};
use futures::future::BoxFuture;
use serde_json::Value;
use tokio::sync::mpsc;
use trex_sandbox::{OpenShell, Sandbox};
use trex_store::library::Library;
use uuid::Uuid;

pub use self::{
    bash::Bash,
    file::{EditFile, ReadFile, WriteFile},
    image::ViewImage,
    library::{LibraryList, LibraryLoad, LibrarySave},
    patch::ApplyPatch,
    plan::UpdatePlan,
    process::{ProcessOutput, StopProcess},
    search::{Glob, Grep},
    time::CurrentTime,
    web::WebFetch,
};
use crate::{event::Event, sandbox::LazySandbox};

// keeps a single noisy result from flooding the model context
pub(crate) const MAX_OUTPUT_BYTES: usize = 32 * 1024;

// a tool only ever reaches the sandbox and library of the user the run belongs to
pub struct ToolContext<'a> {
    pub workspace: Uuid,
    pub library: &'a Library,
    pub openshell: &'a OpenShell,
    pub sandbox: &'a LazySandbox<'a>,
    pub call_id: &'a str,
    pub events: &'a mpsc::Sender<Event>,
}

impl ToolContext<'_> {
    pub async fn sandbox(&self) -> anyhow::Result<&Sandbox> {
        self.sandbox.get(self.events).await
    }
}

pub trait Tool: Send + Sync {
    fn definition(&self) -> FunctionTool;

    fn call<'a>(
        &'a self,
        ctx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, anyhow::Result<String>>;

    // what the model receives; tools whose results include images override it
    fn call_content<'a>(
        &'a self,
        ctx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, anyhow::Result<ToolOutput>> {
        Box::pin(async move { self.call(ctx, args).await.map(ToolOutput::Text) })
    }
}

pub enum ToolOutput {
    Text(String),
    Content(Vec<InputContent>),
}

impl ToolOutput {
    // what the ui and logs show; images and files appear as placeholders
    pub fn text(&self) -> String {
        match self {
            Self::Text(text) => text.clone(),
            Self::Content(parts) => parts
                .iter()
                .map(|part| match part {
                    InputContent::InputText(text) => text.text.clone(),
                    InputContent::InputImage(_) => "[image]".to_owned(),
                    InputContent::InputFile(_) => "[file]".to_owned(),
                })
                .collect::<Vec<_>>()
                .join("\n"),
        }
    }

    pub fn into_function_output(self) -> FunctionCallOutput {
        match self {
            Self::Text(text) => FunctionCallOutput::Text(text),
            Self::Content(parts) => FunctionCallOutput::Content(parts),
        }
    }
}

// a vec keeps tool order stable across requests so the prompt prefix stays cacheable
pub struct Tools {
    tools: Vec<Box<dyn Tool>>,
}

impl Tools {
    pub fn new(tools: Vec<Box<dyn Tool>>) -> Self {
        Self { tools }
    }

    pub fn standard() -> anyhow::Result<Self> {
        Ok(Self::new(vec![
            Box::new(Bash),
            Box::new(ReadFile),
            Box::new(WriteFile),
            Box::new(EditFile),
            Box::new(Grep),
            Box::new(Glob),
            Box::new(LibraryList),
            Box::new(LibraryLoad),
            Box::new(LibrarySave),
            Box::new(WebFetch::new()?),
            Box::new(CurrentTime),
            Box::new(UpdatePlan),
            Box::new(ApplyPatch),
            Box::new(ProcessOutput),
            Box::new(StopProcess),
            Box::new(ViewImage),
        ]))
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

    pub async fn call_content(
        &self,
        ctx: ToolContext<'_>,
        name: &str,
        arguments: &str,
    ) -> anyhow::Result<ToolOutput> {
        let tool = self
            .tools
            .iter()
            .find(|tool| tool.definition().name == name)
            .with_context(|| format!("unknown tool {name}"))?;
        let args = serde_json::from_str(arguments).context("tool arguments are not valid json")?;
        tool.call_content(ctx, args).await
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
    use serde_json::json;

    use super::*;
    use crate::sandbox::LazySandbox;
    use crate::test_support::sandbox_for_new_user;

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and the dev image
    #[tokio::test]
    #[ignore]
    async fn search_and_library_tools_in_sandbox() {
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let (events, _rx) = mpsc::channel(64);
        let tools = Tools::standard().unwrap();
        let call = |name: &'static str, args: Value| {
            let ctx = ToolContext {
                workspace: user,
                library: &library,
                openshell: &openshell,
                sandbox: &sandbox,
                call_id: "call_test",
                events: &events,
            };
            let tools = &tools;
            async move { tools.call(ctx, name, &args.to_string()).await }
        };

        let setup = "mkdir -p /sandbox/proj/src && printf 'fn main() {}\\nfn helper() {}\\n' > /sandbox/proj/src/main.rs \
            && printf 'TODO: ship\\n' > /sandbox/proj/notes.txt && head -c 3000 /dev/urandom > /sandbox/proj/blob.bin";
        let prepared = call("bash", json!({"command": setup})).await.unwrap();

        let grep = call("grep", json!({"pattern": "fn \\w+", "path": "/sandbox/proj", "glob": "*.rs", "ignore_case": null})).await;
        let no_match = call("grep", json!({"pattern": "nothing-here", "path": "/sandbox/proj", "glob": null, "ignore_case": null})).await;
        let glob = call(
            "glob",
            json!({"pattern": "**/*.rs", "path": "/sandbox/proj"}),
        )
        .await;
        let empty = call("library_list", json!({})).await;
        let saved = call(
            "library_save",
            json!({"sandbox_path": "/sandbox/proj/blob.bin", "library_path": "data/blob.bin"}),
        )
        .await;
        let listed = call("library_list", json!({})).await;
        let loaded = call(
            "library_load",
            json!({"library_path": "data/blob.bin", "sandbox_path": null}),
        )
        .await;
        let same = call("bash", json!({"command": "cmp /sandbox/proj/blob.bin /sandbox/library/data/blob.bin && echo same"})).await;
        let escape = call(
            "library_save",
            json!({"sandbox_path": "/sandbox/proj/notes.txt", "library_path": "../other-user/x"}),
        )
        .await;

        openshell.delete_workspace(user).await.unwrap();

        assert!(prepared.contains("[exit code 0]"), "{prepared}");
        assert_eq!(
            grep.unwrap(),
            "/sandbox/proj/src/main.rs:1:fn main() {}\n/sandbox/proj/src/main.rs:2:fn helper() {}\n"
        );
        assert_eq!(no_match.unwrap(), "no matches");
        assert_eq!(glob.unwrap(), "/sandbox/proj/src/main.rs");
        assert_eq!(empty.unwrap(), "the library is empty");
        assert!(saved.unwrap().contains("(3000 bytes)"));
        assert_eq!(listed.unwrap(), "data/blob.bin (3000 bytes)");
        assert!(
            loaded
                .unwrap()
                .ends_with("to /sandbox/library/data/blob.bin")
        );
        assert!(same.unwrap().starts_with("same"));
        assert!(escape.is_err());
        assert_eq!(library.list(user).await.unwrap().len(), 1);
    }

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
