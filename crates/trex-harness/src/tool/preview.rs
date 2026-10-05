use anyhow::{Context, bail};
use async_openai::types::responses::FunctionTool;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext};
use crate::event::Event;

pub struct ShowPreview;

#[derive(Deserialize)]
struct Args {
    port: u16,
    path: Option<String>,
}

impl Tool for ShowPreview {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "show_preview".into(),
            description: Some(
                "Open a web app or site running in the sandbox in the user's preview panel, where they can use it live. \
                 Start the server first (bash with background set), listening on :: or localhost, not 127.0.0.1 only, \
                 for example `vite --host ::` or `python3 -m http.server 8000 --bind ::`. Dev servers with hot reload \
                 keep the preview up to date as you edit."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "port": {"type": "integer", "description": "The port the server listens on."},
                    "path": {"type": ["string", "null"], "description": "Page to open, such as /dashboard; / when null."}
                },
                "required": ["port", "path"],
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
            let args: Args = serde_json::from_value(args).context("invalid arguments")?;
            if args.port == 0 {
                bail!("port must be between 1 and 65535");
            }
            let path = match args.path.as_deref().map(str::trim) {
                Some(path) if path.starts_with('/') => path.to_owned(),
                Some(path) if !path.is_empty() => format!("/{path}"),
                _ => "/".to_owned(),
            };
            // the preview reaches the server over ipv6 loopback, so check it the same way
            let url = format!("http://[::1]:{}{path}", args.port);
            let argv = [
                "curl",
                "-sS",
                "-g",
                "-o",
                "/dev/null",
                "-m",
                "10",
                "-w",
                "%{http_code}",
                &url,
            ]
            .map(String::from)
            .to_vec();
            let output = ctx
                .openshell
                .output(ctx.sandbox().await?, argv, Vec::new())
                .await?;
            if output.exit_code != Some(0) {
                bail!(
                    "nothing answered on [::1]:{} ({}); start the server listening on :: or localhost, then try again",
                    args.port,
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            }
            let status = String::from_utf8_lossy(&output.stdout).trim().to_owned();
            ctx.events
                .send(Event::PreviewOpened {
                    port: args.port,
                    path: path.clone(),
                })
                .await
                .context("event receiver dropped")?;
            Ok(format!(
                "the user's preview panel now shows port {} at {path} (it answered with HTTP {status})",
                args.port
            ))
        })
    }
}
