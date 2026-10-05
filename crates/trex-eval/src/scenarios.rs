use std::{
    fmt::Display,
    path::Path,
    process::Command,
    sync::{Arc, Mutex},
    time::{Duration, Instant},
};

use anyhow::{Context, bail};
use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::time::sleep;

use crate::{
    api::{Api, Watch},
    server::{MODEL, SMALL_CONTEXT_MODEL, Server},
};

pub struct Scenario {
    pub name: &'static str,
    // runs alone, because it restarts the shared trex instance
    pub exclusive: bool,
    pub run: fn(Arc<Ctx>) -> BoxFuture<'static, anyhow::Result<()>>,
}

pub struct Check {
    pub name: String,
    pub passed: bool,
    pub detail: String,
}

pub struct Ctx {
    pub api: Api,
    pub server: Arc<Server>,
    pub effort: Option<String>,
    // library paths of this attempt live under it, so concurrent scenarios never collide
    pub prefix: String,
    pub checks: Mutex<Vec<Check>>,
    pub events: Arc<Mutex<Vec<Value>>>,
    pub sessions: Mutex<Vec<String>>,
}

impl Ctx {
    async fn session(&self, model: &str) -> anyhow::Result<String> {
        let session = self
            .api
            .create_session(model, self.effort.as_deref())
            .await?;
        self.sessions
            .lock()
            .expect("sessions lock poisoned")
            .push(session.clone());
        Ok(session)
    }

    fn watch(&self, session: &str) -> Watch {
        self.api.watch(session, self.events.clone())
    }

    fn check(&self, name: &str, passed: bool, detail: impl Display) {
        self.checks
            .lock()
            .expect("checks lock poisoned")
            .push(Check {
                name: name.to_owned(),
                passed,
                detail: detail.to_string(),
            });
    }

    fn path(&self, name: &str) -> String {
        format!("{}/{name}", self.prefix)
    }

    async fn reply(&self, session: &str) -> anyhow::Result<String> {
        let items = self.api.items(session).await?;
        Ok(items
            .iter()
            .rev()
            .find(|item| item["type"] == "message" && item["role"] == "assistant")
            .and_then(|item| item["text"].as_str())
            .unwrap_or_default()
            .to_owned())
    }

    // sends a message to a fresh session and waits for the run to end
    async fn one_shot(&self, model: &str, prompt: &str) -> anyhow::Result<(String, Watch)> {
        let session = self.session(model).await?;
        let mut watch = self.watch(&session);
        self.api.send(&session, prompt, false).await?;
        let end = watch.until_end().await?;
        self.check_completed(&end);
        Ok((session, watch))
    }

    fn check_completed(&self, end: &Value) {
        let detail = end["error"]
            .as_str()
            .unwrap_or(end["type"].as_str().unwrap_or("?"));
        self.check("run completed", end["type"] == "run.completed", detail);
    }

    fn check_no_sandbox(&self, watch: &Watch) {
        let started = watch.seen.iter().any(|event| {
            event["type"]
                .as_str()
                .is_some_and(|t| t.starts_with("sandbox."))
        });
        self.check("no sandbox started", !started, names(watch));
    }
}

pub fn tool_calls(events: &[Value]) -> Vec<(String, Value)> {
    events
        .iter()
        .filter(|event| event["type"] == "tool.call")
        .map(|event| {
            let arguments = event["arguments"]
                .as_str()
                .and_then(|raw| serde_json::from_str(raw).ok())
                .unwrap_or(Value::Null);
            (event["name"].as_str().unwrap_or("").to_owned(), arguments)
        })
        .collect()
}

fn called(watch: &Watch, tool: &str) -> bool {
    tool_calls(&watch.seen).iter().any(|(name, _)| name == tool)
}

fn names(watch: &Watch) -> String {
    let names: Vec<_> = tool_calls(&watch.seen)
        .into_iter()
        .map(|(name, _)| name)
        .collect();
    format!("{names:?}")
}

fn excerpt(text: &str) -> String {
    let text: String = text.chars().take(200).collect();
    text.replace('\n', " ")
}

macro_rules! scenarios {
    ($($name:ident $(: $exclusive:ident)?),* $(,)?) => {
        pub fn all() -> Vec<Scenario> {
            vec![$(Scenario {
                name: stringify!($name),
                exclusive: scenarios!(@exclusive $($exclusive)?),
                run: |cx| Box::pin($name(cx)),
            }),*]
        }
    };
    (@exclusive exclusive) => { true };
    (@exclusive) => { false };
}

scenarios![
    chat_without_tools,
    current_time,
    run_a_script,
    read_a_web_page,
    fix_a_failing_test,
    analyze_a_csv,
    build_and_package,
    ask_then_continue,
    approve_blocked_network,
    steer_mid_run,
    interrupt_a_long_command,
    compact_a_long_task,
    sandbox_survives_idle_stop,
    resume_after_crash: exclusive,
];

async fn chat_without_tools(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let prompt = "Ugh, my manager moved my deadline up again and I'm exhausted. I just need to vent for a minute.";
    let (session, watch) = cx.one_shot(MODEL, prompt).await?;
    cx.check(
        "no tool calls",
        tool_calls(&watch.seen).is_empty(),
        names(&watch),
    );
    cx.check_no_sandbox(&watch);
    let reply = cx.reply(&session).await?;
    cx.check("replied", !reply.trim().is_empty(), "empty reply");
    Ok(())
}

async fn current_time(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let (session, watch) = cx
        .one_shot(MODEL, "What time is it in Tokyo right now?")
        .await?;
    let asked = tool_calls(&watch.seen)
        .iter()
        .any(|(name, args)| name == "get_current_time" && args["timezone"] == "Asia/Tokyo");
    cx.check("asked for Tokyo time", asked, names(&watch));
    cx.check_no_sandbox(&watch);
    let reply = cx.reply(&session).await?;
    cx.check("gave a time", reply.contains(':'), excerpt(&reply));
    Ok(())
}

async fn run_a_script(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let prompt = "Write a Python script that prints the first 10 Fibonacci numbers, starting from 0, and run it.";
    let (session, watch) = cx.one_shot(MODEL, prompt).await?;
    cx.check("ran bash", called(&watch, "bash"), names(&watch));
    cx.check(
        "kept the script out of the library",
        !called(&watch, "library_save"),
        names(&watch),
    );
    let reply = cx.reply(&session).await?;
    cx.check("showed the output", reply.contains("34"), excerpt(&reply));
    Ok(())
}

async fn read_a_web_page(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let (session, watch) = cx
        .one_shot(
            MODEL,
            "What is the title of the page at https://example.com?",
        )
        .await?;
    cx.check(
        "fetched the page",
        called(&watch, "web_fetch"),
        names(&watch),
    );
    cx.check_no_sandbox(&watch);
    let reply = cx.reply(&session).await?;
    cx.check(
        "found the heading",
        reply.contains("Example Domain"),
        excerpt(&reply),
    );
    Ok(())
}

const BUGGY_MEDIAN: &str = "def median(values):\n    ordered = sorted(values)\n    middle = len(ordered) // 2\n    return ordered[middle]\n";
const MEDIAN_TESTS: &str = "import unittest\n\nfrom calc import median\n\n\nclass MedianTest(unittest.TestCase):\n    def test_odd(self):\n        self.assertEqual(median([3, 1, 2]), 2)\n\n    def test_even(self):\n        self.assertEqual(median([4, 1, 3, 2]), 2.5)\n\n    def test_single(self):\n        self.assertEqual(median([7]), 7)\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n";

async fn fix_a_failing_test(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let (calc, tests) = (cx.path("calc.py"), cx.path("test_calc.py"));
    cx.api.put_file(&calc, BUGGY_MEDIAN.into()).await?;
    cx.api.put_file(&tests, MEDIAN_TESTS.into()).await?;
    let prompt = format!(
        "My library has {calc} and {tests}, and the tests fail. Fix calc.py so they pass without changing \
         the tests, then save the fixed file back to {calc} in my library."
    );
    cx.one_shot(MODEL, &prompt).await?;

    let fixed = cx.api.get_file(&calc).await?.unwrap_or_default();
    let unchanged = cx.api.get_file(&tests).await?.unwrap_or_default();
    cx.check(
        "left the tests alone",
        unchanged == MEDIAN_TESTS.as_bytes(),
        "the tests changed",
    );
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("calc.py"), fixed)?;
    std::fs::write(dir.path().join("test_calc.py"), MEDIAN_TESTS)?;
    let (passed, output) = python(dir.path(), &["-m", "unittest", "-q", "test_calc"])?;
    cx.check("the saved fix passes the tests", passed, excerpt(&output));
    Ok(())
}

async fn analyze_a_csv(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let regions = ["North", "South", "East", "West"];
    let mut csv = String::from("date,region,units,unit_price\n");
    let mut totals = [0u64; 4];
    for day in 0..60u64 {
        let region = (day * 7 % 4) as usize;
        let units = day * 13 % 40 + 1;
        let price = [19, 25, 31, 47][(day % 4) as usize];
        totals[region] += units * price;
        csv.push_str(&format!(
            "2026-{:02}-{:02},{},{units},{price}\n",
            day / 28 + 7,
            day % 28 + 1,
            regions[region]
        ));
    }
    let path = cx.path("sales.csv");
    cx.api.put_file(&path, csv.into()).await?;
    let prompt = format!(
        "Analyze {path} from my library: what is the total revenue (units times unit_price) per region? \
         Reply with a table, largest first."
    );
    let (session, _) = cx.one_shot(MODEL, &prompt).await?;

    let reply = cx.reply(&session).await?;
    let digits: String = reply.chars().filter(|c| *c != ',').collect();
    for (region, total) in regions.iter().zip(totals) {
        cx.check(
            &format!("{region} total is {total}"),
            digits.contains(&total.to_string()),
            excerpt(&reply),
        );
    }
    Ok(())
}

async fn build_and_package(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let archive = cx.path("wordcount.tar.gz");
    let prompt = format!(
        "Build a small Python package in /sandbox/wordcount. wordcount/core.py has count_words(text), which \
         returns a dict of lowercase word counts ignoring punctuation. `python -m wordcount FILE` prints the \
         three most common words with their counts. Add unittest tests in a tests/ directory and run them. \
         Then save a tar.gz of the project to my library as {archive}, with the wordcount/ package and tests/ \
         at the top level of the archive."
    );
    cx.one_shot(MODEL, &prompt).await?;

    let Some(bytes) = cx.api.get_file(&archive).await? else {
        cx.check("saved the archive", false, "no archive in the library");
        return Ok(());
    };
    let dir = tempfile::tempdir()?;
    std::fs::write(dir.path().join("project.tar.gz"), bytes)?;
    let extracted = Command::new("tar")
        .args(["xzf", "project.tar.gz"])
        .current_dir(dir.path())
        .status()?;
    cx.check("the archive extracts", extracted.success(), "tar failed");
    let root = find_package_root(dir.path()).context("no wordcount/core.py in the archive")?;
    let script = "from wordcount.core import count_words\n\
        assert count_words('The cat, the hat. THE end!') == {'the': 3, 'cat': 1, 'hat': 1, 'end': 1}, count_words('The cat, the hat. THE end!')";
    let (works, output) = python(&root, &["-c", script])?;
    cx.check("count_words is correct", works, excerpt(&output));
    let (passed, output) = python(
        &root,
        &["-m", "unittest", "discover", "-s", "tests", "-t", "."],
    )?;
    let ran_some = output.contains("Ran ") && !output.contains("Ran 0 tests");
    cx.check("its own tests pass", passed && ran_some, excerpt(&output));
    Ok(())
}

async fn ask_then_continue(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let session = cx.session(MODEL).await?;
    let mut watch = cx.watch(&session);
    let prompt = "I want a script that prints exactly `hello from the sandbox`. Before writing anything, use \
        ask_user to ask me which language to use, offering Python and Node as options. Then write it, run it, \
        and reply with only what it printed.";
    cx.api.send(&session, prompt, false).await?;
    let end = watch.until_end().await?;
    cx.check(
        "asked a question",
        end["type"] == "run.needs_input",
        &end["type"],
    );
    let state = cx.api.session(&session).await?;
    let python = state["pending_questions"][0]["options"]
        .as_array()
        .into_iter()
        .flatten()
        .filter_map(|option| option["label"].as_str())
        .find(|label| label.to_lowercase().contains("python"))
        .map(str::to_owned);
    cx.check(
        "offered Python",
        python.is_some(),
        &state["pending_questions"],
    );
    let Some(python) = python else {
        return Ok(());
    };

    cx.api
        .answer(&session, json!([{"selected": [python], "text": null}]))
        .await?;
    let end = watch.until_end().await?;
    cx.check_completed(&end);
    let used_python = tool_calls(&watch.seen)
        .iter()
        .any(|(_, args)| args.to_string().contains("python"));
    cx.check("used Python", used_python, names(&watch));
    let reply = cx.reply(&session).await?;
    cx.check(
        "printed the greeting",
        reply.contains("hello from the sandbox"),
        excerpt(&reply),
    );
    Ok(())
}

async fn approve_blocked_network(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let session = cx.session(MODEL).await?;
    let mut watch = cx.watch(&session);
    cx.api
        .send(
            &session,
            "Use bash to run `curl -sS --max-time 10 https://example.com` and tell me what happened.",
            false,
        )
        .await?;
    let end = watch.until_end().await?;
    cx.check_completed(&end);

    // denials are reported in batches, so one raised late in the run may only reach the list
    let mut request = None;
    for _ in 0..10 {
        let requests = cx.api.access_requests(&session).await?;
        request = requests
            .into_iter()
            .find(|request| request["endpoints"].to_string().contains("example.com"));
        if request.is_some() {
            break;
        }
        sleep(Duration::from_secs(2)).await;
    }
    cx.check(
        "raised an access request",
        request.is_some(),
        "no request for example.com",
    );
    let Some(request) = request else {
        return Ok(());
    };
    let id = request["id"].as_str().context("access request has no id")?;
    cx.api.approve(&session, id).await?;

    cx.api
        .send(
            &session,
            "I approved it. Run the same curl again and tell me the page title.",
            false,
        )
        .await?;
    let end = watch.until_end().await?;
    cx.check_completed(&end);
    let reply = cx.reply(&session).await?;
    cx.check(
        "reached the site after approval",
        reply.contains("Example Domain"),
        excerpt(&reply),
    );
    Ok(())
}

async fn steer_mid_run(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let session = cx.session(MODEL).await?;
    let mut watch = cx.watch(&session);
    cx.api
        .send(
            &session,
            "Use bash to run `sleep 15 && echo first-step-done`, then reply with a one-line summary.",
            false,
        )
        .await?;
    watch.until_type("tool.call").await?;
    let queued = cx
        .api
        .send(
            &session,
            "Also, end your reply with the word PINEAPPLE.",
            false,
        )
        .await?;
    cx.check("the message was queued", queued["queued"] == true, &queued);
    let end = watch.until_end().await?;
    cx.check_completed(&end);
    let received = watch
        .seen
        .iter()
        .any(|event| event["type"] == "message.received");
    cx.check("the agent received it", received, "no message.received");
    let reply = cx.reply(&session).await?;
    cx.check(
        "followed the new instruction",
        reply.contains("PINEAPPLE"),
        excerpt(&reply),
    );
    Ok(())
}

async fn interrupt_a_long_command(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let session = cx.session(MODEL).await?;
    let mut watch = cx.watch(&session);
    cx.api
        .send(
            &session,
            "Use bash to run `sleep 120 && echo finished`, then tell me what it printed.",
            false,
        )
        .await?;
    watch.until_type("tool.call").await?;
    sleep(Duration::from_secs(3)).await;
    let sent = Instant::now();
    cx.api
        .send(
            &session,
            "Stop that, I changed my mind. Reply with only the word stopped.",
            true,
        )
        .await?;
    watch.until_type("run.interrupted").await?;
    let reaction = sent.elapsed();
    cx.check(
        "interrupted quickly",
        reaction < Duration::from_secs(15),
        format!("{reaction:.1?}"),
    );
    let end = watch.until_end().await?;
    cx.check_completed(&end);
    let reply = cx.reply(&session).await?;
    cx.check(
        "replied to the interruption",
        reply.to_lowercase().contains("stopped"),
        excerpt(&reply),
    );
    Ok(())
}

async fn compact_a_long_task(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let prompt = "My code word is ORCHID-7. Run these three commands with three separate bash calls, one at a \
        time: `seq 1 2000`, `seq 2001 4000`, `seq 4001 6000`. Then reply with only my code word.";
    let (session, watch) = cx.one_shot(SMALL_CONTEXT_MODEL, prompt).await?;
    let compacted = watch
        .seen
        .iter()
        .any(|event| event["type"] == "context.compacted");
    cx.check("compacted the context", compacted, "no context.compacted");
    let reply = cx.reply(&session).await?;
    cx.check(
        "remembered the code word",
        reply.contains("ORCHID-7"),
        excerpt(&reply),
    );
    Ok(())
}

async fn sandbox_survives_idle_stop(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let session = cx.session(MODEL).await?;
    let mut watch = cx.watch(&session);
    cx.api
        .send(
            &session,
            "Use bash to write the word persisted into /sandbox/keep.txt, then reply ok.",
            false,
        )
        .await?;
    let end = watch.until_end().await?;
    cx.check_completed(&end);

    // the idle timeout is 15s and the reaper runs every minute
    sleep(Duration::from_secs(80)).await;
    let before = watch.seen.len();
    cx.api
        .send(
            &session,
            "Use bash to print /sandbox/keep.txt and reply with only its contents.",
            false,
        )
        .await?;
    let end = watch.until_end().await?;
    cx.check_completed(&end);
    let restarted = watch.seen[before..]
        .iter()
        .any(|event| event["type"] == "sandbox.starting");
    cx.check(
        "restarted the stopped sandbox",
        restarted,
        "no sandbox.starting",
    );
    let reply = cx.reply(&session).await?;
    cx.check(
        "the file survived",
        reply.contains("persisted"),
        excerpt(&reply),
    );
    Ok(())
}

async fn resume_after_crash(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let session = cx.session(MODEL).await?;
    let mut watch = cx.watch(&session);
    cx.api
        .send(
            &session,
            "Use bash to run `sleep 30 && echo phase-one-done`. After it finishes or fails, use bash to run \
             `echo phase-two-done`. Then reply with only the word finished.",
            false,
        )
        .await?;
    watch.until_type("tool.call").await?;
    sleep(Duration::from_secs(3)).await;
    cx.server.kill().await?;
    cx.server.restart().await?;

    let end = watch.until_end().await?;
    cx.check_completed(&end);
    let resumed = watch
        .seen
        .iter()
        .any(|event| event["type"] == "run.resumed");
    cx.check("resumed the run", resumed, "no run.resumed");
    let phase_two = tool_calls(&watch.seen)
        .iter()
        .any(|(_, args)| args.to_string().contains("phase-two-done"));
    cx.check("carried on with the task", phase_two, names(&watch));
    let reply = cx.reply(&session).await?;
    cx.check(
        "finished",
        reply.to_lowercase().contains("finished"),
        excerpt(&reply),
    );
    Ok(())
}

fn python(dir: &Path, args: &[&str]) -> anyhow::Result<(bool, String)> {
    let output = Command::new("python3")
        .args(args)
        .current_dir(dir)
        .output()
        .context("failed to run python3")?;
    let text = format!(
        "{}{}",
        String::from_utf8_lossy(&output.stdout),
        String::from_utf8_lossy(&output.stderr)
    );
    Ok((output.status.success(), text))
}

fn find_package_root(dir: &Path) -> anyhow::Result<std::path::PathBuf> {
    for entry in walk(dir)? {
        if entry.ends_with("wordcount/core.py") {
            let root = entry
                .parent()
                .and_then(Path::parent)
                .context("core.py has no package directory")?;
            return Ok(root.to_owned());
        }
    }
    bail!("no wordcount/core.py found")
}

fn walk(dir: &Path) -> anyhow::Result<Vec<std::path::PathBuf>> {
    let mut files = Vec::new();
    for entry in std::fs::read_dir(dir)? {
        let path = entry?.path();
        if path.is_dir() {
            files.extend(walk(&path)?);
        } else {
            files.push(path);
        }
    }
    Ok(files)
}
