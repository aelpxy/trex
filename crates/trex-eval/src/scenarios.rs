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
use tokio::time::{sleep, timeout};

const CREDITS_EMAIL: &str = "eval-credits@trex.local";

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
    projects_and_titles,
    stops_when_out_of_credits,
    current_time,
    run_a_script,
    read_a_web_page,
    describe_an_attached_image,
    read_an_attached_pdf,
    use_an_attached_file,
    check_a_picture_it_drew,
    fix_a_failing_test,
    rename_across_files,
    analyze_a_csv,
    build_and_package,
    ask_then_continue,
    approve_blocked_network,
    serve_in_the_background,
    preview_a_site,
    run_a_long_job,
    steer_mid_run,
    edit_in_a_branch,
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
    cx.check(
        "didn't plan a quick task",
        !called(&watch, "update_plan"),
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

// 64x48, left half blue, right half yellow
const SPLIT_PNG: &str = "iVBORw0KGgoAAAANSUhEUgAAAEAAAAAwCAIAAAAuKetIAAAAS0lEQVR42u3PMQ0AMAgAMCRMAZomfQJmgx8LPHxNaqCR96+qd1aFgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgICAgMBUA/Czuw/DYXFZAAAAAElFTkSuQmCC";
// one page: "Recipe notes: the secret ingredient is cardamom."
const RECIPE_PDF: &str = "JVBERi0xLjQKMSAwIG9iago8PCAvVHlwZSAvQ2F0YWxvZyAvUGFnZXMgMiAwIFIgPj4KZW5kb2JqCjIgMCBvYmoKPDwgL1R5cGUgL1BhZ2VzIC9LaWRzIFszIDAgUl0gL0NvdW50IDEgPj4KZW5kb2JqCjMgMCBvYmoKPDwgL1R5cGUgL1BhZ2UgL1BhcmVudCAyIDAgUiAvTWVkaWFCb3ggWzAgMCA2MTIgNzkyXSAvQ29udGVudHMgNCAwIFIgL1Jlc291cmNlcyA8PCAvRm9udCA8PCAvRjEgNSAwIFIgPj4gPj4gPj4KZW5kb2JqCjQgMCBvYmoKPDwgL0xlbmd0aCA3OSA+PgpzdHJlYW0KQlQgL0YxIDE4IFRmIDcyIDcyMCBUZCAoUmVjaXBlIG5vdGVzOiB0aGUgc2VjcmV0IGluZ3JlZGllbnQgaXMgY2FyZGFtb20uKSBUaiBFVAplbmRzdHJlYW0KZW5kb2JqCjUgMCBvYmoKPDwgL1R5cGUgL0ZvbnQgL1N1YnR5cGUgL1R5cGUxIC9CYXNlRm9udCAvSGVsdmV0aWNhID4+CmVuZG9iagp4cmVmCjAgNgowMDAwMDAwMDAwIDY1NTM1IGYgCjAwMDAwMDAwMDkgMDAwMDAgbiAKMDAwMDAwMDA1OCAwMDAwMCBuIAowMDAwMDAwMTE1IDAwMDAwIG4gCjAwMDAwMDAyNDEgMDAwMDAgbiAKMDAwMDAwMDM3MCAwMDAwMCBuIAp0cmFpbGVyCjw8IC9TaXplIDYgL1Jvb3QgMSAwIFIgPj4Kc3RhcnR4cmVmCjQ0MAolJUVPRgo=";

async fn describe_an_attached_image(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let session = cx.session(MODEL).await?;
    let mut watch = cx.watch(&session);
    let data = format!("data:image/png;base64,{SPLIT_PNG}");
    let attachments = json!([{"data": data, "filename": null, "library_path": null}]);
    let question = "What color is the left half of this image, and what color is the right half? Reply only \
        in the form `left: COLOR, right: COLOR`.";
    cx.api
        .send_with(&session, question, false, attachments)
        .await?;
    let end = watch.until_end().await?;
    cx.check_completed(&end);
    cx.check_no_sandbox(&watch);
    let reply = cx.reply(&session).await?.to_lowercase();
    let seen = reply.contains("left: blue") && reply.contains("right: yellow");
    cx.check(
        "saw blue on the left and yellow on the right",
        seen,
        excerpt(&reply),
    );

    let items = cx.api.items(&session).await?;
    let attached: Vec<Value> = items
        .iter()
        .filter_map(|item| item["attachments"].as_array())
        .flatten()
        .cloned()
        .collect();
    let listed = attached.len() == 1 && attached[0]["kind"] == "image";
    cx.check("the item lists the image", listed, format!("{attached:?}"));
    let id = attached
        .first()
        .and_then(|a| a["id"].as_str())
        .unwrap_or_default();
    let downloaded = cx.api.attachment(id).await.unwrap_or_default();
    cx.check(
        "the attachment downloads intact",
        downloaded == decode_base64(SPLIT_PNG),
        format!("{} bytes", downloaded.len()),
    );
    Ok(())
}

async fn read_an_attached_pdf(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let session = cx.session(MODEL).await?;
    let mut watch = cx.watch(&session);
    let data = format!("data:application/pdf;base64,{RECIPE_PDF}");
    let attachments = json!([{"data": data, "filename": "recipe.pdf", "library_path": null}]);
    let question = "What is the secret ingredient in these notes? Reply with just the ingredient.";
    cx.api
        .send_with(&session, question, false, attachments)
        .await?;
    let end = watch.until_end().await?;
    cx.check_completed(&end);
    cx.check_no_sandbox(&watch);
    let reply = cx.reply(&session).await?;
    cx.check(
        "read the pdf",
        reply.to_lowercase().contains("cardamom"),
        excerpt(&reply),
    );
    Ok(())
}

async fn use_an_attached_file(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let session = cx.session(MODEL).await?;
    let mut watch = cx.watch(&session);
    let csv = "region,amount\nnorth,1250\nsouth,830\neast,2417\nwest,96\n";
    let data = format!("data:text/csv;base64,{}", encode_base64(csv.as_bytes()));
    let attachments = json!([{"data": data, "filename": "sales.csv", "library_path": null}]);
    let question = "Use Python in the sandbox to add up the amount column of the attached sales.csv. Reply with just the total.";
    cx.api
        .send_with(&session, question, false, attachments)
        .await?;
    let end = watch.until_end().await?;
    cx.check_completed(&end);

    let used_upload = tool_calls(&watch.seen)
        .iter()
        .any(|(_, args)| args.to_string().contains("/sandbox/uploads/sales.csv"));
    cx.check("read the upload in the sandbox", used_upload, names(&watch));
    let reply = cx.reply(&session).await?;
    cx.check(
        "summed the column",
        reply.replace(',', "").contains("4593"),
        excerpt(&reply),
    );

    let files = cx.api.get(&format!("/sessions/{session}/files")).await?;
    let listed = files["data"]
        .as_array()
        .is_some_and(|files| files.iter().any(|file| file["path"] == "uploads/sales.csv"));
    cx.check(
        "the files api lists the upload",
        listed,
        files.to_string().chars().take(200).collect::<String>(),
    );
    let items = cx.api.items(&session).await?;
    let shown = items
        .iter()
        .find(|item| item["type"] == "message" && item["role"] == "user")
        .and_then(|item| item["text"].as_str())
        .unwrap_or_default();
    cx.check(
        "the sandbox note stays hidden",
        shown == question,
        excerpt(shown),
    );
    Ok(())
}

async fn check_a_picture_it_drew(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let prompt = "Pick one color from red, green or purple, then use Python to draw a large filled circle in \
        that color on a white background and save it as /sandbox/circle.png. Then look at the image with \
        view_image and tell me which color the circle is.";
    let (session, watch) = cx.one_shot(MODEL, prompt).await?;
    cx.check(
        "looked at the image",
        called(&watch, "view_image"),
        names(&watch),
    );
    let drawn: String = tool_calls(&watch.seen)
        .iter()
        .filter(|(name, _)| name != "view_image")
        .map(|(_, args)| args.to_string().to_lowercase())
        .collect();
    let reply = cx.reply(&session).await?.to_lowercase();
    let matches = ["red", "green", "purple"]
        .iter()
        .any(|color| drawn.contains(color) && reply.contains(color));
    cx.check("named the color it drew", matches, excerpt(&reply));
    Ok(())
}

// the eval doesn't otherwise need a base64 crate, and these fixtures are tiny
fn encode_base64(bytes: &[u8]) -> String {
    const ALPHABET: &[u8; 64] = b"ABCDEFGHIJKLMNOPQRSTUVWXYZabcdefghijklmnopqrstuvwxyz0123456789+/";
    let mut out = String::new();
    for chunk in bytes.chunks(3) {
        let value = chunk.iter().enumerate().fold(0u32, |value, (index, byte)| {
            value | (u32::from(*byte) << (16 - 8 * index))
        });
        for index in 0..4 {
            if index <= chunk.len() {
                out.push(ALPHABET[(value >> (18 - 6 * index) & 63) as usize] as char);
            } else {
                out.push('=');
            }
        }
    }
    out
}

fn decode_base64(text: &str) -> Vec<u8> {
    let mut out = Vec::new();
    let (mut buffer, mut bits) = (0u32, 0);
    for byte in text.bytes().filter(|byte| *byte != b'=') {
        let value = match byte {
            b'A'..=b'Z' => byte - b'A',
            b'a'..=b'z' => byte - b'a' + 26,
            b'0'..=b'9' => byte - b'0' + 52,
            b'+' => 62,
            _ => 63,
        };
        buffer = (buffer << 6) | u32::from(value);
        bits += 6;
        if bits >= 8 {
            bits -= 8;
            out.push((buffer >> bits) as u8);
        }
    }
    out
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

const SHAPES: &str = "def area(width, height):\n    return width * height\n";
const REPORT: &str = "from shapes import area\n\n\ndef describe(width, height):\n    return f\"{width}x{height} has area {area(width, height)}\"\n";
const SHAPE_TESTS: &str = "import unittest\n\nfrom report import describe\nfrom shapes import area\n\n\nclass ShapesTest(unittest.TestCase):\n    def test_area(self):\n        self.assertEqual(area(2, 3), 6)\n\n    def test_describe(self):\n        self.assertEqual(describe(2, 3), \"2x3 has area 6\")\n\n\nif __name__ == \"__main__\":\n    unittest.main()\n";

async fn rename_across_files(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let files = [
        ("shapes.py", SHAPES),
        ("report.py", REPORT),
        ("test_shapes.py", SHAPE_TESTS),
    ];
    for (name, content) in files {
        cx.api.put_file(&cx.path(name), content.into()).await?;
    }
    let dir = cx.path("");
    let prompt = format!(
        "My library has a small Python project in {dir}: shapes.py, report.py and test_shapes.py. Rename the \
         function area to rectangle_area everywhere it is defined or used, including the tests, make sure the \
         tests pass, then save all three files back to the same paths in my library."
    );
    let (_, watch) = cx.one_shot(MODEL, &prompt).await?;
    let changed = watch
        .seen
        .iter()
        .filter(|event| {
            event["type"] == "file.changed"
                && event["diff"]
                    .as_str()
                    .is_some_and(|d| d.contains("rectangle_area"))
        })
        .count();
    cx.check(
        "reported the edits as diffs",
        changed >= 3,
        format!("{changed} file.changed events"),
    );

    let local = tempfile::tempdir()?;
    let mut leftovers = Vec::new();
    for (name, _) in files {
        let content = cx.api.get_file(&cx.path(name)).await?.unwrap_or_default();
        let text = String::from_utf8_lossy(&content).replace("rectangle_area", "");
        if text.contains("area(") || text.contains("import area") {
            leftovers.push(name);
        }
        std::fs::write(local.path().join(name), content)?;
    }
    cx.check(
        "renamed every use",
        leftovers.is_empty(),
        format!("{leftovers:?}"),
    );
    let (passed, output) = python(local.path(), &["-m", "unittest", "-q", "test_shapes"])?;
    cx.check(
        "the saved project passes its tests",
        passed,
        excerpt(&output),
    );
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
    let (session, watch) = cx.one_shot(MODEL, &prompt).await?;
    let reply = cx.reply(&session).await?;
    let last_plan = watch
        .seen
        .iter()
        .rev()
        .find(|event| event["type"] == "plan.updated");
    cx.check("kept a plan", last_plan.is_some(), names(&watch));
    let finished = last_plan.is_some_and(|plan| {
        plan["steps"]
            .as_array()
            .is_some_and(|steps| steps.iter().all(|step| step["status"] == "completed"))
    });
    cx.check(
        "finished every step of the plan",
        finished,
        last_plan
            .map(|plan| plan["steps"].to_string())
            .unwrap_or_default(),
    );

    let Some(bytes) = cx.api.get_file(&archive).await? else {
        cx.check(
            "saved the archive",
            false,
            format!("no archive; reply: {}", excerpt(&reply)),
        );
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

fn started_in_background(watch: &Watch) -> bool {
    tool_calls(&watch.seen)
        .iter()
        .any(|(name, args)| name == "bash" && args["background"] == true)
}

async fn serve_in_the_background(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let prompt = "Create /sandbox/site/index.html containing the text hello from the background, serve /sandbox/site \
        with Python's http.server on port 8000, fetch the page with curl, and tell me what it returned. Stop the \
        server when you're done.";
    let (session, watch) = cx.one_shot(MODEL, prompt).await?;
    cx.check(
        "ran the server in the background",
        started_in_background(&watch),
        names(&watch),
    );
    cx.check(
        "stopped the server",
        called(&watch, "stop_process"),
        names(&watch),
    );
    let reply = cx.reply(&session).await?;
    cx.check(
        "fetched the page",
        reply.contains("hello from the background"),
        excerpt(&reply),
    );
    Ok(())
}

async fn preview_a_site(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let prompt = "Create /sandbox/site/index.html containing the text preview-check-7731, serve /sandbox/site with \
        Python's http.server on port 8000, and show it to me in the preview.";
    let (session, watch) = cx.one_shot(MODEL, prompt).await?;
    cx.check(
        "opened the preview",
        called(&watch, "show_preview"),
        names(&watch),
    );
    let opened = watch
        .seen
        .iter()
        .any(|event| event["type"] == "preview.opened" && event["port"] == 8000);
    cx.check("announced port 8000", opened, names(&watch));

    let preview = cx
        .api
        .post(
            &format!("/sessions/{session}/previews"),
            json!({"port": 8000}),
        )
        .await?;
    let url = preview["url"].as_str().unwrap_or_default();
    let host = url
        .split_once("://")
        .and_then(|(_, rest)| rest.split('/').next())
        .unwrap_or_default();
    let page = reqwest::Client::new()
        .get(format!("http://{}/", cx.server.preview_addr()))
        .header("host", host)
        .send()
        .await?
        .text()
        .await
        .unwrap_or_default();
    cx.check(
        "the preview serves the page",
        page.contains("preview-check-7731"),
        excerpt(&page),
    );
    Ok(())
}

async fn run_a_long_job(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let prompt = "Run `sleep 130 && echo slow-job-done` (it takes a little over two minutes) and tell me what it printed.";
    let (session, watch) = cx.one_shot(MODEL, prompt).await?;
    cx.check(
        "ran it in the background",
        started_in_background(&watch),
        names(&watch),
    );
    let reply = cx.reply(&session).await?;
    cx.check(
        "waited for the result",
        reply.contains("slow-job-done"),
        excerpt(&reply),
    );
    Ok(())
}

async fn edit_in_a_branch(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let session = cx.session(MODEL).await?;
    let mut watch = cx.watch(&session);
    cx.api
        .send(
            &session,
            "My favorite color is green. Reply with just OK.",
            false,
        )
        .await?;
    watch.until_end().await?;
    cx.api
        .send(
            &session,
            "What is my favorite color? Reply with one word.",
            false,
        )
        .await?;
    watch.until_end().await?;

    let branch = cx
        .api
        .post(
            &format!("/sessions/{session}/branch"),
            json!({"message": 0, "content": "My favorite color is purple. Reply with just OK."}),
        )
        .await?;
    let branch = branch["id"].as_str().unwrap_or_default().to_owned();
    cx.sessions
        .lock()
        .expect("sessions lock poisoned")
        .push(branch.clone());
    let mut branch_watch = cx.watch(&branch);
    let end = branch_watch.until_end().await?;
    cx.check_completed(&end);
    cx.api
        .send(
            &branch,
            "What is my favorite color? Reply with one word.",
            false,
        )
        .await?;
    branch_watch.until_end().await?;

    let edited = cx.reply(&branch).await?.to_lowercase();
    cx.check(
        "the branch follows the edit",
        edited.contains("purple"),
        excerpt(&edited),
    );
    let original = cx.reply(&session).await?.to_lowercase();
    cx.check(
        "the original is untouched",
        original.contains("green"),
        excerpt(&original),
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

async fn projects_and_titles(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let project = cx
        .api
        .post(
            "/projects",
            json!({"name": format!("Eval {}", cx.prefix), "instructions": "End every reply with the word OKAPI in capitals."}),
        )
        .await?;
    let project_id = project["id"]
        .as_str()
        .context("project has no id")?
        .to_owned();
    let created = cx
        .api
        .post(
            "/sessions",
            json!({"model": MODEL, "reasoning_effort": cx.effort, "project_id": project_id}),
        )
        .await?;
    let session = created["id"]
        .as_str()
        .context("session has no id")?
        .to_owned();
    cx.sessions
        .lock()
        .expect("sessions lock poisoned")
        .push(session.clone());
    cx.check(
        "started in the project",
        created["project_id"] == project_id.as_str(),
        &created["project_id"],
    );

    let mut watch = cx.watch(&session);
    cx.api
        .send(&session, "Suggest one name for a pet turtle.", false)
        .await?;
    let end = watch.until_end().await?;
    cx.check_completed(&end);
    let reply = cx.reply(&session).await?;
    cx.check(
        "followed the project instructions",
        reply.contains("OKAPI"),
        excerpt(&reply),
    );

    let titled = timeout(Duration::from_secs(30), async {
        loop {
            if watch
                .seen
                .iter()
                .any(|event| event["type"] == "session.updated")
            {
                break;
            }
            watch.next().await?;
        }
        anyhow::Ok(())
    })
    .await;
    let title = cx.api.session(&session).await?["title"].clone();
    cx.check(
        "titled the chat",
        titled.is_ok()
            && title
                .as_str()
                .is_some_and(|t| !t.is_empty() && t.len() < 80),
        &title,
    );

    let listed = |filter: &str| {
        let api = cx.api.clone();
        let filter = filter.to_owned();
        async move {
            let page = api
                .get(&format!("/sessions?project_id={filter}&limit=100"))
                .await?;
            anyhow::Ok(
                page["data"]
                    .as_array()
                    .into_iter()
                    .flatten()
                    .filter_map(|s| s["id"].as_str().map(str::to_owned))
                    .collect::<Vec<_>>(),
            )
        }
    };
    let in_project = listed(&project_id).await?;
    let recents = listed("none").await?;
    cx.check(
        "listed under the project, not in recents",
        in_project == [session.clone()] && !recents.contains(&session),
        format!("{in_project:?}"),
    );

    let moved = cx
        .api
        .patch(
            &format!("/sessions/{session}"),
            json!({"title": "Turtle names", "project_id": null}),
        )
        .await?;
    let recents = listed("none").await?;
    cx.check(
        "renamed and moved to recents",
        moved["title"] == "Turtle names"
            && moved["project_id"].is_null()
            && recents.contains(&session),
        &moved,
    );
    cx.api.delete(&format!("/projects/{project_id}")).await?;
    Ok(())
}

async fn stops_when_out_of_credits(cx: Arc<Ctx>) -> anyhow::Result<()> {
    let api = Api::account(cx.server.url(), CREDITS_EMAIL, crate::EVAL_PASSWORD).await?;
    let admin = Api::admin(cx.server.url(), cx.server.admin_token.clone());
    let me = api.get("/me").await?;
    let workspace = me["workspaces"][0]["id"]
        .as_str()
        .context("no workspace")?
        .to_owned();
    let balance = api.get("/credits").await?["balance"]
        .as_i64()
        .context("no balance")?;
    admin
        .post(
            &format!("/admin/workspaces/{workspace}/credits"),
            json!({"amount": 1 - balance, "description": "eval: one credit left"}),
        )
        .await?;

    let session = api.create_session(MODEL, cx.effort.as_deref()).await?;
    let mut watch = api.watch(&session, cx.events.clone());
    api.send(
        &session,
        "Use bash to run `echo one`. After that, use bash to run `echo two`. Then reply with only the word done.",
        false,
    )
    .await?;
    let end = watch.until_end().await?;
    cx.check(
        "stopped for lack of credits",
        end["type"] == "run.failed" && end["code"] == "insufficient_credits",
        &end,
    );
    let responses = watch
        .seen
        .iter()
        .filter(|event| event["type"] == "usage")
        .count();
    cx.check(
        "finished only the step it had started",
        responses == 1,
        format!("{responses} responses"),
    );
    let (status, body) = api.try_send(&session, "Are you still there?").await?;
    cx.check(
        "refused the next message",
        status.as_u16() == 402 && body["error"]["type"] == "insufficient_credits_error",
        format!("{status} {body}"),
    );
    let ledger = api.get("/credits/ledger?limit=5").await?;
    let charged = ledger["data"][0]["kind"] == "usage"
        && ledger["data"][0]["amount"].as_i64().is_some_and(|a| a < 0);
    cx.check(
        "charged the response in the ledger",
        charged,
        &ledger["data"][0],
    );
    api.delete_session(&session).await?;
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
