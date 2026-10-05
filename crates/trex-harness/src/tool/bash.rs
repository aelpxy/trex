use std::time::Duration;

use anyhow::Context;
use async_openai::types::responses::FunctionTool;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};
use tokio::time::timeout;
use trex_sandbox::ExecEvent;

use super::{Tool, ToolContext, process, truncate};
use crate::event::{Event, OutputStream};

const BASH_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Bash;

#[derive(Deserialize)]
struct Args {
    command: String,
    background: Option<bool>,
}

impl Tool for Bash {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "bash".into(),
            description: Some(
                "Run a bash command in the sandbox and return its stdout, stderr and exit code. Commands time out \
                 after 120 seconds; set background for servers, watchers and longer jobs, then use process_output. \
                 Use read_file, write_file, edit_file and apply_patch for file contents instead of cat, echo or sed. \
                 Network access is limited to an allowlist; if a host is blocked, tell the user which host you need \
                 and why, since they can approve it, instead of retrying or working around the block."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "command": {"type": "string", "description": "The bash command to run."},
                    "background": {"type": ["boolean", "null"], "description": "Start the command in the background and return at once with a process id."}
                },
                "required": ["command", "background"],
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
            let args: Args = serde_json::from_value(args)?;
            if args.background == Some(true) {
                return process::start(&ctx, &args.command).await;
            }
            let argv = vec!["bash".to_owned(), "-c".to_owned(), args.command];
            let mut stream = ctx
                .openshell
                .exec(ctx.sandbox().await?, argv, Vec::new())
                .await?;

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
