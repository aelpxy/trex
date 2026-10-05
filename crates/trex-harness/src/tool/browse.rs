use std::time::Duration;

use anyhow::{Context, bail};
use async_openai::types::responses::{
    FunctionTool, ImageDetail, InputContent, InputImageContent, InputTextContent,
};
use futures::future::BoxFuture;
use serde::{Deserialize, Serialize};
use serde_json::{Value, json};
use tokio::time::timeout;

use super::{Tool, ToolContext, ToolOutput, file::read_bytes};
use crate::attachment;

// the script runs in the sandbox with the playwright the image provides, sent with every call so
// changing it never needs a new image
const SCRIPT: &str = include_str!("browse.mjs");
const SCREENSHOT_DIR: &str = "/tmp/.browse";
// loading, every step and the screenshot, with room to spare
const BROWSE_TIMEOUT: Duration = Duration::from_secs(120);

pub struct Browse;

#[derive(Deserialize)]
struct Args {
    url: Option<String>,
    port: Option<u16>,
    path: Option<String>,
    steps: Option<Vec<Step>>,
    viewport: Option<String>,
}

#[derive(Deserialize, Serialize)]
struct Step {
    action: String,
    target: Option<String>,
    text: Option<String>,
}

#[derive(Serialize)]
struct Request<'a> {
    url: &'a str,
    steps: &'a [Step],
    viewport: Option<&'a str>,
    directory: &'a str,
    id: &'a str,
}

// what the script saw; unknown fields are ignored so the script can grow
#[derive(Deserialize, Default)]
#[serde(default)]
struct Report {
    unavailable: bool,
    error: Option<String>,
    url: Option<String>,
    title: Option<String>,
    outline: Option<String>,
    screenshot: Option<String>,
    steps: Vec<String>,
    console: Vec<String>,
    failed: Vec<String>,
}

impl Tool for Browse {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "browse".into(),
            description: Some(
                "Open a page in a headless browser in the sandbox and see it: returns a screenshot, console errors, \
                 failed requests and an outline of the page's elements by role and name. Use it after building or \
                 changing a web UI to check it renders and works, and to read pages that need JavaScript. For a \
                 server running in the sandbox pass its port. Steps run in order; a target is a role and name from \
                 the outline like `button \"Sign in\"`, a CSS selector, or visible text."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "url": {"type": ["string", "null"], "description": "The page to open; null when using port."},
                    "port": {"type": ["integer", "null"], "description": "A port a server in the sandbox listens on; opens http://localhost:<port>."},
                    "path": {"type": ["string", "null"], "description": "The path to open with port, like /settings; / when null."},
                    "steps": {
                        "type": ["array", "null"],
                        "description": "What to do on the page before the screenshot, in order.",
                        "items": {
                            "type": "object",
                            "properties": {
                                "action": {"type": "string", "enum": ["click", "type", "press", "wait_for_text", "scroll", "wait"]},
                                "target": {"type": ["string", "null"], "description": "The element for click and type."},
                                "text": {"type": ["string", "null"], "description": "Text to type, the key to press (like Enter), text to wait for, pixels to scroll or milliseconds to wait."}
                            },
                            "required": ["action", "target", "text"],
                            "additionalProperties": false
                        }
                    },
                    "viewport": {"type": ["string", "null"], "enum": ["desktop", "mobile", null], "description": "Screen size; desktop when null."}
                },
                "required": ["url", "port", "path", "steps", "viewport"],
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
        Box::pin(async move { Ok(self.call_content(ctx, args).await?.text()) })
    }

    fn call_content<'a>(
        &'a self,
        ctx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, anyhow::Result<ToolOutput>> {
        Box::pin(async move {
            let args: Args = serde_json::from_value(args)?;
            let url = target_url(&args)?;
            let steps = args.steps.unwrap_or_default();
            let id: String = ctx
                .call_id
                .chars()
                .filter(char::is_ascii_alphanumeric)
                .collect();
            let request = serde_json::to_string(&Request {
                url: &url,
                steps: &steps,
                viewport: args.viewport.as_deref(),
                directory: SCREENSHOT_DIR,
                id: &id,
            })?;
            let argv = ["node", "--input-type=module", "-", &request]
                .map(String::from)
                .to_vec();
            let output = timeout(
                BROWSE_TIMEOUT,
                ctx.openshell
                    .output(ctx.sandbox().await?, argv, SCRIPT.as_bytes().to_vec()),
            )
            .await
            .with_context(|| format!("the browser took over {}s", BROWSE_TIMEOUT.as_secs()))??;
            let stdout = String::from_utf8_lossy(&output.stdout);
            let Some(report) = stdout
                .lines()
                .rev()
                .find_map(|line| serde_json::from_str::<Report>(line).ok())
            else {
                bail!(
                    "the browser failed: {}",
                    String::from_utf8_lossy(&output.stderr).trim()
                );
            };
            if report.unavailable {
                bail!(
                    "this sandbox was made before the browser was added, so it has none; it's in sandboxes for new chats"
                );
            }

            let mut parts = vec![InputContent::InputText(InputTextContent {
                text: describe(&report),
                prompt_cache_breakpoint: None,
            })];
            if let Some(path) = &report.screenshot {
                let bytes = read_bytes(&ctx, path).await?;
                let mime =
                    attachment::image_mime(&bytes).context("the screenshot isn't an image")?;
                let hash = ctx.library.put_attachment(ctx.workspace, bytes).await?;
                parts.push(InputContent::InputImage(InputImageContent {
                    detail: ImageDetail::Auto,
                    file_id: None,
                    image_url: Some(attachment::reference(&hash, mime)),
                    prompt_cache_breakpoint: None,
                }));
            }
            Ok(ToolOutput::Content(parts))
        })
    }
}

// a port means a server in the sandbox, which answers on localhost; ipv4 loopback doesn't work there
fn target_url(args: &Args) -> anyhow::Result<String> {
    let url = match (args.port, &args.url) {
        (Some(port), _) => {
            let path = args.path.as_deref().unwrap_or("/");
            let path = if path.starts_with('/') {
                path.to_owned()
            } else {
                format!("/{path}")
            };
            format!("http://localhost:{port}{path}")
        }
        (None, Some(url)) => url.replace("://127.0.0.1", "://localhost"),
        (None, None) => bail!("give a url or the port of a server in the sandbox"),
    };
    if !url.starts_with("http://") && !url.starts_with("https://") {
        bail!("only http and https pages can be opened");
    }
    Ok(url)
}

// the text the model reads beside the screenshot
fn describe(report: &Report) -> String {
    let mut text = format!(
        "{} — {}",
        report
            .title
            .as_deref()
            .filter(|title| !title.is_empty())
            .unwrap_or("(no title)"),
        report.url.as_deref().unwrap_or("(no page)")
    );
    if let Some(error) = &report.error {
        text.push_str(&format!("\n\nCouldn't finish loading: {error}"));
    }
    let list = |title: &str, items: &[String], empty: &str| {
        if items.is_empty() {
            format!("\n\n{empty}")
        } else {
            format!(
                "\n\n{title}:\n{}",
                items
                    .iter()
                    .map(|item| format!("- {item}"))
                    .collect::<Vec<_>>()
                    .join("\n")
            )
        }
    };
    if !report.steps.is_empty() {
        text.push_str(&list("Steps", &report.steps, ""));
    }
    text.push_str(&list(
        "Console errors and warnings",
        &report.console,
        "No console errors.",
    ));
    text.push_str(&list(
        "Failed requests",
        &report.failed,
        "No failed requests.",
    ));
    if let Some(outline) = report
        .outline
        .as_deref()
        .filter(|outline| !outline.is_empty())
    {
        text.push_str(&format!("\n\nPage outline:\n{outline}"));
    }
    text
}

#[cfg(test)]
mod tests {
    use super::*;

    fn args(url: Option<&str>, port: Option<u16>, path: Option<&str>) -> Args {
        Args {
            url: url.map(str::to_owned),
            port,
            path: path.map(str::to_owned),
            steps: None,
            viewport: None,
        }
    }

    #[test]
    fn opens_sandbox_servers_on_localhost() {
        assert_eq!(
            target_url(&args(None, Some(5173), None)).unwrap(),
            "http://localhost:5173/"
        );
        assert_eq!(
            target_url(&args(None, Some(3000), Some("settings"))).unwrap(),
            "http://localhost:3000/settings"
        );
        assert_eq!(
            target_url(&args(Some("http://127.0.0.1:8000/a"), None, None)).unwrap(),
            "http://localhost:8000/a"
        );
        assert!(target_url(&args(Some("file:///etc/passwd"), None, None)).is_err());
        assert!(target_url(&args(None, None, None)).is_err());
    }

    #[test]
    fn describes_what_the_page_showed() {
        let report = Report {
            title: Some("Counter".into()),
            url: Some("http://localhost:8765/".into()),
            steps: vec!["click button \"Say hi\": ok".into()],
            console: vec!["error: boom".into()],
            outline: Some("- button \"Say hi\"".into()),
            ..Report::default()
        };
        assert_eq!(
            describe(&report),
            "Counter — http://localhost:8765/\n\nSteps:\n- click button \"Say hi\": ok\n\nConsole errors and warnings:\n- error: boom\
             \n\nNo failed requests.\n\nPage outline:\n- button \"Say hi\""
        );
    }

    // needs the openshell gateway tunnel, certs, and the dev image with the browser
    #[tokio::test]
    #[ignore]
    async fn sees_and_clicks_a_page_in_the_sandbox() {
        use tokio::sync::mpsc;
        use trex_store::library::Library;

        use crate::{sandbox::LazySandbox, test_support::sandbox_for_new_user, tool::Tools};

        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let page = r#"<!doctype html><meta charset=utf-8><title>Reveal</title><button onclick="s.hidden=false">Reveal</button><p id=s hidden>secret-7731</p>"#;
        let serve = format!(
            "mkdir -p /sandbox/site && printf '%s' '{page}' > /sandbox/site/index.html && (setsid python3 -m http.server 8000 --bind :: --directory /sandbox/site >/dev/null 2>&1 &) && sleep 1"
        );
        openshell
            .output(
                &sandbox,
                ["bash", "-c", &serve].map(String::from).to_vec(),
                Vec::new(),
            )
            .await
            .unwrap();
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let (events, _rx) = mpsc::channel(16);
        let tools = Tools::standard().unwrap();
        let ctx = ToolContext {
            workspace: user,
            library: &library,
            openshell: &openshell,
            sandbox: &sandbox,
            call_id: "call_browse",
            events: &events,
            scheduler: None,
        };
        let args = json!({"url": null, "port": 8000, "path": null, "viewport": null,
            "steps": [{"action": "click", "target": "button \"Reveal\"", "text": null}]});
        let result = tools.call_content(ctx, "browse", &args.to_string()).await;

        openshell.delete_workspace(user).await.unwrap();

        let output = result.unwrap();
        let text = output.text();
        assert!(text.contains("click button \"Reveal\": ok"), "{text}");
        assert!(text.contains("secret-7731"), "{text}");
        assert!(text.contains("[image: attachment://"), "{text}");
    }
}
