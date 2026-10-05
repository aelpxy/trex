mod api;
mod scenarios;
mod server;

use std::{
    path::Path,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use futures::{StreamExt, stream};
use serde_json::{Value, json};
use tokio::time::timeout;
use uuid::Uuid;

use crate::{
    api::Api,
    scenarios::{Ctx, Scenario, tool_calls},
    server::Server,
};

const USAGE: &str = "usage: cargo run -p trex-eval -- [--repeat N] [--concurrency N] [--effort LEVEL] [SCENARIO...]";
const SCENARIO_TIMEOUT: Duration = Duration::from_secs(600);
// one fixed user, so eval runs reuse a single openshell workspace instead of leaving one behind each
const EVAL_USER: Uuid = Uuid::from_u128(0x0000_0000_0000_7000_8000_0000_0000_e7a1);

struct Options {
    repeat: usize,
    concurrency: usize,
    effort: Option<String>,
    filters: Vec<String>,
}

struct Outcome {
    name: &'static str,
    attempt: usize,
    error: Option<String>,
    checks: Vec<scenarios::Check>,
    seconds: f64,
    tool_calls: usize,
    tools: Vec<String>,
    input_tokens: u64,
    cached_tokens: u64,
    output_tokens: u64,
}

impl Outcome {
    fn passed(&self) -> bool {
        self.error.is_none() && !self.checks.is_empty() && self.checks.iter().all(|c| c.passed)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    let options = parse_args()?;
    let root = Path::new(env!("CARGO_MANIFEST_DIR")).join("../..");
    let root = root
        .canonicalize()
        .context("failed to find the workspace root")?;

    let selected: Vec<Scenario> = scenarios::all()
        .into_iter()
        .filter(|s| {
            options.filters.is_empty()
                || options.filters.iter().any(|f| s.name.contains(f.as_str()))
        })
        .collect();
    if selected.is_empty() {
        bail!("no scenario matches {:?}", options.filters);
    }

    println!(
        "starting trex for {} scenarios x{}",
        selected.len(),
        options.repeat
    );
    let server = Arc::new(Server::start(&root).await?);
    let api = Api::new(server.url(), EVAL_USER);
    clear_library(&api).await?;

    let started = Instant::now();
    let (shared, exclusive): (Vec<_>, Vec<_>) = selected.iter().partition(|s| !s.exclusive);
    let mut outcomes: Vec<Outcome> = stream::iter(jobs(shared, options.repeat))
        .map(|(scenario, attempt)| run(scenario, attempt, &api, &server, &options))
        .buffer_unordered(options.concurrency)
        .collect()
        .await;
    for (scenario, attempt) in jobs(exclusive, options.repeat) {
        outcomes.push(run(scenario, attempt, &api, &server, &options).await);
    }
    clear_library(&api).await?;
    server.kill().await?;

    outcomes.sort_by_key(|o| (selected.iter().position(|s| s.name == o.name), o.attempt));
    report(&outcomes, started.elapsed());
    let path = root.join("target/eval/last-run.json");
    std::fs::write(&path, serde_json::to_string_pretty(&to_json(&outcomes))?)?;
    println!(
        "details: {}  server log: {}",
        path.display(),
        server.log_path().display()
    );

    if outcomes.iter().any(|o| !o.passed()) {
        std::process::exit(1);
    }
    Ok(())
}

fn jobs(scenarios: Vec<&Scenario>, repeat: usize) -> Vec<(&Scenario, usize)> {
    scenarios
        .into_iter()
        .flat_map(|s| (1..=repeat).map(move |attempt| (s, attempt)))
        .collect()
}

fn parse_args() -> anyhow::Result<Options> {
    let mut options = Options {
        repeat: 1,
        concurrency: 4,
        effort: None,
        filters: Vec::new(),
    };
    let mut args = std::env::args().skip(1);
    while let Some(arg) = args.next() {
        let mut value = || {
            args.next()
                .with_context(|| format!("{arg} needs a value\n{USAGE}"))
        };
        match arg.as_str() {
            "--repeat" => options.repeat = value()?.parse().context("invalid --repeat")?,
            "--concurrency" => {
                options.concurrency = value()?.parse().context("invalid --concurrency")?
            }
            "--effort" => options.effort = Some(value()?),
            "-h" | "--help" => {
                println!("{USAGE}");
                std::process::exit(0);
            }
            filter if !filter.starts_with('-') => options.filters.push(filter.to_owned()),
            other => bail!("unknown option {other}\n{USAGE}"),
        }
    }
    if options.repeat == 0 || options.concurrency == 0 {
        bail!("--repeat and --concurrency must be at least 1");
    }
    Ok(options)
}

async fn run(
    scenario: &Scenario,
    attempt: usize,
    api: &Api,
    server: &Arc<Server>,
    options: &Options,
) -> Outcome {
    let cx = Arc::new(Ctx {
        api: api.clone(),
        server: server.clone(),
        effort: options.effort.clone(),
        prefix: format!(
            "eval/{}-{attempt}-{}",
            scenario.name,
            Uuid::now_v7().simple()
        ),
        checks: Mutex::new(Vec::new()),
        events: Arc::new(Mutex::new(Vec::new())),
        sessions: Mutex::new(Vec::new()),
    });
    let started = Instant::now();
    let result = timeout(SCENARIO_TIMEOUT, (scenario.run)(cx.clone())).await;
    let seconds = started.elapsed().as_secs_f64();
    let error = match result {
        Ok(Ok(())) => None,
        Ok(Err(error)) => Some(format!("{error:#}")),
        Err(_) => Some(format!("timed out after {}s", SCENARIO_TIMEOUT.as_secs())),
    };

    let sessions = cx.sessions.lock().expect("sessions lock poisoned").clone();
    for session in sessions {
        if let Err(error) = api.delete_session(&session).await {
            eprintln!("could not delete {session}: {error:#}");
        }
    }
    let events = cx.events.lock().expect("event sink lock poisoned").clone();
    let usage = |field: &str| -> u64 {
        events
            .iter()
            .filter(|e| e["type"] == "usage")
            .filter_map(|e| e[field].as_u64())
            .sum()
    };
    let outcome = Outcome {
        name: scenario.name,
        attempt,
        error,
        checks: std::mem::take(&mut *cx.checks.lock().expect("checks lock poisoned")),
        seconds,
        tool_calls: tool_calls(&events).len(),
        tools: tool_calls(&events)
            .into_iter()
            .map(|(name, args)| {
                let args: String = args.to_string().chars().take(240).collect();
                format!("{name} {args}")
            })
            .collect(),
        input_tokens: usage("input_tokens"),
        cached_tokens: usage("cached_input_tokens"),
        output_tokens: usage("output_tokens"),
    };
    println!(
        "{} {} #{attempt} ({seconds:.0}s)",
        if outcome.passed() { "pass" } else { "FAIL" },
        scenario.name
    );
    outcome
}

async fn clear_library(api: &Api) -> anyhow::Result<()> {
    for path in api.files().await? {
        api.delete_file(&path).await?;
    }
    Ok(())
}

fn report(outcomes: &[Outcome], elapsed: Duration) {
    println!();
    println!(
        "{:<28} {:>6} {:>7} {:>6} {:>10} {:>8} {:>8}",
        "scenario", "result", "time", "tools", "input", "cached", "output"
    );
    for o in outcomes {
        println!(
            "{:<28} {:>6} {:>6.0}s {:>6} {:>10} {:>8} {:>8}",
            if o.attempt > 1 {
                format!("{} #{}", o.name, o.attempt)
            } else {
                o.name.to_owned()
            },
            if o.passed() { "pass" } else { "FAIL" },
            o.seconds,
            o.tool_calls,
            o.input_tokens,
            o.cached_tokens,
            o.output_tokens
        );
    }

    let failed: Vec<_> = outcomes.iter().filter(|o| !o.passed()).collect();
    for o in &failed {
        println!("\n{} #{}:", o.name, o.attempt);
        if let Some(error) = &o.error {
            println!("  error: {error}");
        }
        for check in o.checks.iter().filter(|c| !c.passed) {
            println!("  failed: {} ({})", check.name, check.detail);
        }
    }
    let passed = outcomes.len() - failed.len();
    let input: u64 = outcomes.iter().map(|o| o.input_tokens).sum();
    let output: u64 = outcomes.iter().map(|o| o.output_tokens).sum();
    println!(
        "\n{passed}/{} passed in {:.0}s, {input} input and {output} output tokens",
        outcomes.len(),
        elapsed.as_secs_f64()
    );
}

fn to_json(outcomes: &[Outcome]) -> Value {
    json!(outcomes
        .iter()
        .map(|o| json!({
            "scenario": o.name,
            "attempt": o.attempt,
            "passed": o.passed(),
            "error": o.error,
            "seconds": o.seconds,
            "tool_calls": o.tool_calls,
            "tools": o.tools,
            "input_tokens": o.input_tokens,
            "cached_input_tokens": o.cached_tokens,
            "output_tokens": o.output_tokens,
            "checks": o.checks.iter().map(|c| json!({"name": c.name, "passed": c.passed, "detail": c.detail})).collect::<Vec<_>>(),
        }))
        .collect::<Vec<_>>())
}
