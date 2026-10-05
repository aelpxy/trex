use anyhow::{Context, bail};
use async_openai::types::responses::FunctionTool;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};
use uuid::Uuid;

use super::{MAX_OUTPUT_BYTES, Tool, ToolContext};

const MAX_WAIT_SECONDS: u64 = 60;
const MISSING_EXIT_CODE: i32 = 3;

// a background process runs in its own session so it outlives the exec that started it; its output
// and exit code live in files, so any later run in the conversation can read them
const START: &str = r#"
dir="/tmp/.processes/$1"
mkdir -p "$dir" || exit 1
printf '%s' "$2" > "$dir/command"
: > "$dir/output"
setsid bash -c "$3" _ "$2" "$dir" > /dev/null 2>&1 < /dev/null &
echo $! > "$dir/pid"
"#;

// each exec is sandboxed separately and can't signal processes another one started, so the
// wrapper stops its own process group when asked through a file
const WRAPPER: &str = r#"
bash -c "$1" > "$2/output" 2>&1 < /dev/null &
child=$!
trap '' TERM
while kill -0 "$child" 2>/dev/null; do
    if [ -f "$2/stop" ]; then
        kill -TERM -- "-$$" 2>/dev/null
        for _ in 1 2 3 4 5 6 7 8 9 10; do kill -0 "$child" 2>/dev/null || break; sleep 0.5; done
        : > "$2/stopped"
        kill -KILL -- "-$$"
    fi
    sleep 0.5
done
wait "$child"
echo $? > "$2/exit"
"#;

const STATUS: &str = r#"
status() {
    if [ -f "$1/stopped" ]; then echo "stopped"
    elif [ -f "$1/exit" ]; then echo "exited with code $(cat "$1/exit")"
    elif [ -d "/proc/$(cat "$1/pid")" ]; then echo "running"
    else echo "stopped"; fi
}
"#;

const READ: &str = r#"
dir="/tmp/.processes/$1"
[ -d "$dir" ] || exit 3
offset=$(cat "$dir/read" 2>/dev/null || echo 0)
waited=0
while [ "$waited" -lt "$2" ] && [ "$(stat -c %s "$dir/output")" -le "$offset" ] && [ "$(status "$dir")" = running ]; do
    sleep 1
    waited=$((waited + 1))
done
size=$(stat -c %s "$dir/output")
echo "status: $(status "$dir")"
start=$offset
if [ $((size - offset)) -gt "$3" ]; then
    start=$((size - $3))
    echo "[... $((start - offset)) earlier bytes skipped ...]"
fi
tail -c +$((start + 1)) "$dir/output" | head -c $((size - start))
echo "$size" > "$dir/read"
"#;

const LIST: &str = r#"
for dir in /tmp/.processes/*/; do
    [ -d "$dir" ] || continue
    dir=${dir%/}
    echo "$(basename "$dir")  $(status "$dir")  $(head -n 1 "$dir/command" | cut -c 1-120)"
done
"#;

const STOP: &str = r#"
dir="/tmp/.processes/$1"
[ -d "$dir" ] || exit 3
if [ "$(status "$dir")" != running ]; then echo "it had already $(status "$dir")"; exit 0; fi
: > "$dir/stop"
for _ in $(seq 1 20); do [ "$(status "$dir")" = running ] || break; sleep 0.5; done
status "$dir"
"#;

pub struct ProcessOutput;
pub struct StopProcess;

#[derive(Deserialize)]
struct OutputArgs {
    id: Option<String>,
    wait_seconds: Option<u64>,
}

#[derive(Deserialize)]
struct StopArgs {
    id: String,
}

pub(super) async fn start(ctx: &ToolContext<'_>, command: &str) -> anyhow::Result<String> {
    let id = format!("p{}", &Uuid::now_v7().simple().to_string()[24..]);
    script(ctx, START, &[&id, command, WRAPPER]).await?;
    Ok(format!(
        "started background process {id}; read its output with process_output and stop it with stop_process"
    ))
}

impl Tool for ProcessOutput {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "process_output".into(),
            description: Some(format!(
                "Read what a background process printed since you last read it, and whether it is still running. \
                 wait_seconds (up to {MAX_WAIT_SECONDS}) waits for new output or for the process to exit. Without an \
                 id, lists the background processes."
            )),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "id": {"type": ["string", "null"], "description": "The process id from bash with background set."},
                    "wait_seconds": {"type": ["integer", "null"], "description": "How long to wait for new output; 0 when null."}
                },
                "required": ["id", "wait_seconds"],
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
            let args: OutputArgs = serde_json::from_value(args)?;
            let Some(id) = args.id else {
                let list = script(&ctx, &format!("{STATUS}{LIST}"), &[]).await?;
                return Ok(if list.trim().is_empty() {
                    "no background processes".into()
                } else {
                    list
                });
            };
            let wait = args.wait_seconds.unwrap_or(0).min(MAX_WAIT_SECONDS);
            script(
                &ctx,
                &format!("{STATUS}{READ}"),
                &[&id, &wait.to_string(), &MAX_OUTPUT_BYTES.to_string()],
            )
            .await
            .with_context(|| format!("no background process {id}"))
        })
    }
}

impl Tool for StopProcess {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "stop_process".into(),
            description: Some("Stop a background process and everything it started.".into()),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "id": {"type": "string", "description": "The process id."}
                },
                "required": ["id"],
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
            let args: StopArgs = serde_json::from_value(args)?;
            script(&ctx, &format!("{STATUS}{STOP}"), &[&args.id])
                .await
                .with_context(|| format!("no background process {}", args.id))
        })
    }
}

// arguments are passed as $1.. so they are never interpreted by the shell
async fn script(ctx: &ToolContext<'_>, script: &str, args: &[&str]) -> anyhow::Result<String> {
    let mut argv = vec![
        "bash".to_owned(),
        "-c".to_owned(),
        script.to_owned(),
        "bash".to_owned(),
    ];
    argv.extend(args.iter().map(|arg| (*arg).to_owned()));
    let output = ctx
        .openshell
        .output(ctx.sandbox().await?, argv, Vec::new())
        .await?;
    match output.exit_code {
        Some(0) => Ok(String::from_utf8_lossy(&output.stdout).into_owned()),
        Some(MISSING_EXIT_CODE) => bail!("not found"),
        _ => bail!("{}", String::from_utf8_lossy(&output.stderr).trim()),
    }
}

#[cfg(test)]
mod tests {
    use std::time::{Duration, Instant};

    use serde_json::json;
    use tokio::sync::mpsc;
    use trex_store::library::Library;

    use crate::{sandbox::LazySandbox, test_support::sandbox_for_new_user, tool::Tools};

    use super::*;

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and the dev image
    #[tokio::test]
    #[ignore]
    async fn runs_processes_in_the_background() {
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let tools = Tools::standard().unwrap();
        let (events, _rx) = mpsc::channel(64);
        let call = |name: &'static str, args: Value| {
            let ctx = ToolContext {
                workspace: user,
                library: &library,
                openshell: &openshell,
                sandbox: &sandbox,
                call_id: "call_test",
                events: &events,
                scheduler: None,
            };
            let tools = &tools;
            async move { tools.call(ctx, name, &args.to_string()).await.unwrap() }
        };
        let id_of = |started: &str| {
            started
                .split_whitespace()
                .nth(3)
                .unwrap()
                .trim_end_matches(';')
                .to_owned()
        };

        let started_at = Instant::now();
        let ticker = call(
            "bash",
            json!({"command": "for i in 1 2 3; do echo tick $i; sleep 1; done; exit 4", "background": true}),
        )
        .await;
        let returned_in = started_at.elapsed();
        let ticker = id_of(&ticker);
        let server =
            id_of(&call("bash", json!({"command": "sleep 300", "background": true})).await);

        let first = call("process_output", json!({"id": ticker, "wait_seconds": 10})).await;
        let (mut rest, mut last) = (String::new(), String::new());
        for _ in 0..10 {
            last = call("process_output", json!({"id": ticker, "wait_seconds": 5})).await;
            rest.push_str(&last);
            if last.contains("exited") {
                break;
            }
        }
        let running = call(
            "process_output",
            json!({"id": server, "wait_seconds": null}),
        )
        .await;
        let listed = call("process_output", json!({"id": null, "wait_seconds": null})).await;
        let stopped = call("stop_process", json!({"id": server})).await;
        let after = call(
            "process_output",
            json!({"id": server, "wait_seconds": null}),
        )
        .await;
        let missing = tools
            .call(
                ToolContext {
                    workspace: user,
                    library: &library,
                    openshell: &openshell,
                    sandbox: &sandbox,
                    call_id: "call_test",
                    events: &events,
                    scheduler: None,
                },
                "process_output",
                &json!({"id": "nope", "wait_seconds": null}).to_string(),
            )
            .await;
        openshell.delete_workspace(user).await.unwrap();

        assert!(returned_in < Duration::from_secs(5), "{returned_in:?}");
        assert!(first.starts_with("status: running\ntick 1"), "{first}");
        assert!(last.starts_with("status: exited with code 4"), "{last}");
        assert!(
            rest.contains("tick 3") && !rest.contains("tick 1"),
            "only new output: {rest}"
        );
        assert_eq!(running, "status: running\n");
        assert!(
            listed.contains(&ticker) && listed.contains(&format!("{server}  running  sleep 300")),
            "{listed}"
        );
        assert_eq!(stopped, "stopped\n");
        assert_eq!(after, "status: stopped\n");
        assert!(
            missing
                .unwrap_err()
                .to_string()
                .contains("no background process nope")
        );
    }
}
