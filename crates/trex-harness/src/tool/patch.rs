use anyhow::{Context, bail};
use async_openai::types::responses::FunctionTool;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Tool, ToolContext,
    file::{file_changed, read, read_optional, remove, write},
};
use crate::event::FileChange;

const WORKDIR: &str = "/sandbox";
const BEGIN: &str = "*** Begin Patch";
const END: &str = "*** End Patch";
const ADD: &str = "*** Add File: ";
const DELETE: &str = "*** Delete File: ";
const UPDATE: &str = "*** Update File: ";
const MOVE: &str = "*** Move to: ";
const END_OF_FILE: &str = "*** End of File";

pub struct ApplyPatch;

#[derive(Deserialize)]
struct Args {
    patch: String,
}

#[derive(Debug, PartialEq)]
enum Hunk {
    Add {
        path: String,
        content: String,
    },
    Delete {
        path: String,
    },
    Update {
        path: String,
        move_to: Option<String>,
        chunks: Vec<Chunk>,
    },
}

#[derive(Debug, Default, PartialEq)]
struct Chunk {
    // the line after @@, found first to place the chunk
    anchor: Option<String>,
    old: Vec<String>,
    new: Vec<String>,
    end_of_file: bool,
}

// a fully planned change, so nothing is written unless every hunk applies
struct Change {
    path: String,
    from: Option<String>,
    old: Option<String>,
    new: Option<String>,
}

impl Tool for ApplyPatch {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "apply_patch".into(),
            description: Some(format!(
                "Create, edit, move and delete files in the sandbox with one patch, the best way to make related \
                 changes across files. Nothing is changed unless every part applies. Format:\n\
                 {BEGIN}\n\
                 {ADD}path/to/new.py\n\
                 +every line of the new file, each prefixed with +\n\
                 {UPDATE}path/to/existing.py\n\
                 {MOVE}path/to/renamed.py   (optional)\n\
                 @@ def some_function():   (optional line to locate the change)\n\
                 \x20context line, prefixed with a space\n\
                 -removed line\n\
                 +added line\n\
                 {DELETE}path/to/old.py\n\
                 {END}\n\
                 Give about 3 unchanged lines of context around each change and start a new @@ section for each \
                 separate place in a file. Relative paths are relative to {WORKDIR}."
            )),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "patch": {"type": "string", "description": "The whole patch, from *** Begin Patch to *** End Patch."}
                },
                "required": ["patch"],
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
            let hunks = parse(&args.patch)?;
            let changes = plan(&ctx, hunks).await?;
            let mut summary = Vec::new();
            for change in &changes {
                summary.push(commit(&ctx, change).await?);
            }
            Ok(summary.join("\n"))
        })
    }
}

async fn plan(ctx: &ToolContext<'_>, hunks: Vec<Hunk>) -> anyhow::Result<Vec<Change>> {
    let mut changes = Vec::new();
    for hunk in hunks {
        let change = match hunk {
            Hunk::Add { path, content } => {
                let path = resolve(&path);
                let old = read_optional(ctx, &path)
                    .await?
                    .map(|bytes| String::from_utf8_lossy(&bytes).into_owned());
                Change {
                    path,
                    from: None,
                    old,
                    new: Some(content),
                }
            }
            Hunk::Delete { path } => {
                let path = resolve(&path);
                let old = read(ctx, &path)
                    .await
                    .with_context(|| format!("cannot delete {path}"))?;
                Change {
                    path,
                    from: None,
                    old: Some(old),
                    new: None,
                }
            }
            Hunk::Update {
                path,
                move_to,
                chunks,
            } => {
                let path = resolve(&path);
                let old = read(ctx, &path)
                    .await
                    .with_context(|| format!("cannot update {path}"))?;
                let new = apply_chunks(&path, &old, &chunks)?;
                match move_to {
                    Some(to) => Change {
                        path: resolve(&to),
                        from: Some(path),
                        old: Some(old),
                        new: Some(new),
                    },
                    None => Change {
                        path,
                        from: None,
                        old: Some(old),
                        new: Some(new),
                    },
                }
            }
        };
        changes.push(change);
    }
    Ok(changes)
}

async fn commit(ctx: &ToolContext<'_>, change: &Change) -> anyhow::Result<String> {
    let Change {
        path,
        from,
        old,
        new,
    } = change;
    let (kind, line) = match new {
        None => {
            remove(ctx, path).await?;
            (FileChange::Deleted, format!("D {path}"))
        }
        Some(content) => {
            write(ctx, path, content.as_bytes()).await?;
            match from {
                Some(from) => {
                    remove(ctx, from).await?;
                    (
                        FileChange::Moved { from: from.clone() },
                        format!("R {from} -> {path}"),
                    )
                }
                None if old.is_none() => (FileChange::Added, format!("A {path}")),
                None => (FileChange::Updated, format!("M {path}")),
            }
        }
    };
    file_changed(ctx, path, kind, old.as_deref(), new.as_deref()).await?;
    Ok(line)
}

fn resolve(path: &str) -> String {
    if path.starts_with('/') {
        path.to_owned()
    } else {
        format!("{WORKDIR}/{path}")
    }
}

fn parse(patch: &str) -> anyhow::Result<Vec<Hunk>> {
    let lines: Vec<&str> = patch.lines().collect();
    let begin = lines
        .iter()
        .position(|line| line.trim() == BEGIN)
        .with_context(|| format!("the patch must start with {BEGIN}"))?;
    let end = lines
        .iter()
        .rposition(|line| line.trim() == END)
        .with_context(|| format!("the patch must end with {END}"))?;
    if end < begin {
        bail!("{END} comes before {BEGIN}");
    }
    let body = &lines[begin + 1..end];

    let mut hunks = Vec::new();
    let mut i = 0;
    while i < body.len() {
        let line = body[i].trim_end();
        i += 1;
        if let Some(path) = line.strip_prefix(ADD) {
            let mut content = String::new();
            while i < body.len() && !body[i].starts_with("*** ") {
                let Some(text) = body[i].strip_prefix('+') else {
                    bail!(
                        "every line of an added file must start with +, found {:?} in {}",
                        body[i],
                        path.trim()
                    );
                };
                content.push_str(text);
                content.push('\n');
                i += 1;
            }
            hunks.push(Hunk::Add {
                path: path.trim().to_owned(),
                content,
            });
        } else if let Some(path) = line.strip_prefix(DELETE) {
            hunks.push(Hunk::Delete {
                path: path.trim().to_owned(),
            });
        } else if let Some(path) = line.strip_prefix(UPDATE) {
            let path = path.trim().to_owned();
            let mut move_to = None;
            if let Some(to) = body
                .get(i)
                .and_then(|line| line.trim_end().strip_prefix(MOVE))
            {
                move_to = Some(to.trim().to_owned());
                i += 1;
            }
            let mut chunks = Vec::new();
            let mut chunk: Option<Chunk> = None;
            while i < body.len() {
                let line = body[i];
                if line.trim_end() == END_OF_FILE {
                    chunk.get_or_insert_with(Chunk::default).end_of_file = true;
                } else if line.starts_with("*** ") {
                    break;
                } else if let Some(anchor) = line.strip_prefix("@@") {
                    chunks.extend(chunk.take());
                    let anchor = anchor.trim();
                    chunk = Some(Chunk {
                        anchor: (!anchor.is_empty()).then(|| anchor.to_owned()),
                        ..Default::default()
                    });
                } else {
                    let current = chunk.get_or_insert_with(Chunk::default);
                    match line.chars().next() {
                        Some(' ') => {
                            current.old.push(line[1..].to_owned());
                            current.new.push(line[1..].to_owned());
                        }
                        Some('-') => current.old.push(line[1..].to_owned()),
                        Some('+') => current.new.push(line[1..].to_owned()),
                        // models often drop the space prefix of a blank context line
                        None => {
                            current.old.push(String::new());
                            current.new.push(String::new());
                        }
                        Some(_) => bail!(
                            "in the update of {path}, {line:?} must start with a space, - or +"
                        ),
                    }
                }
                i += 1;
            }
            chunks.extend(chunk);
            if chunks.is_empty() && move_to.is_none() {
                bail!("the update of {path} has no changes");
            }
            hunks.push(Hunk::Update {
                path,
                move_to,
                chunks,
            });
        } else if !line.trim().is_empty() {
            bail!("unexpected line {line:?}; expected {ADD}, {UPDATE} or {DELETE}");
        }
    }
    if hunks.is_empty() {
        bail!("the patch changes nothing");
    }
    Ok(hunks)
}

fn apply_chunks(path: &str, content: &str, chunks: &[Chunk]) -> anyhow::Result<String> {
    let mut lines: Vec<String> = content.lines().map(str::to_owned).collect();
    let mut edits: Vec<(usize, usize, Vec<String>)> = Vec::new();
    let mut cursor = 0;
    for chunk in chunks {
        if let Some(anchor) = &chunk.anchor {
            let at = seek(&lines, std::slice::from_ref(anchor), cursor, false)
                .with_context(|| format!("could not find the line {anchor:?} in {path}"))?;
            cursor = at + 1;
        }
        if chunk.old.is_empty() {
            let at = if chunk.anchor.is_some() {
                cursor
            } else {
                lines.len()
            };
            edits.push((at, 0, chunk.new.clone()));
            continue;
        }
        let (mut old, mut new) = (chunk.old.as_slice(), chunk.new.as_slice());
        let mut found = seek(&lines, old, cursor, chunk.end_of_file);
        // a trailing blank context line often has no counterpart at the end of the file
        if found.is_none() && old.last().is_some_and(String::is_empty) {
            old = &old[..old.len() - 1];
            if new.last().is_some_and(String::is_empty) {
                new = &new[..new.len() - 1];
            }
            found = seek(&lines, old, cursor, chunk.end_of_file);
        }
        let at = found.with_context(|| {
            format!(
                "could not find these lines in {path}; read the file and try again:\n{}",
                chunk.old.join("\n")
            )
        })?;
        edits.push((at, old.len(), new.to_vec()));
        cursor = at + old.len();
    }
    for (at, len, new) in edits.into_iter().rev() {
        lines.splice(at..at + len, new);
    }
    let mut out = lines.join("\n");
    if !out.is_empty() {
        out.push('\n');
    }
    Ok(out)
}

// exact matches win, then matches ignoring trailing and then surrounding whitespace
fn seek(lines: &[String], pattern: &[String], start: usize, end_of_file: bool) -> Option<usize> {
    if pattern.len() > lines.len() || start > lines.len() - pattern.len() {
        return None;
    }
    let last = lines.len() - pattern.len();
    let comparisons: [fn(&str, &str) -> bool; 3] = [
        |a, b| a == b,
        |a, b| a.trim_end() == b.trim_end(),
        |a, b| a.trim() == b.trim(),
    ];
    for same in comparisons {
        let matches = |at: usize| {
            pattern
                .iter()
                .zip(&lines[at..])
                .all(|(want, have)| same(want, have))
        };
        if end_of_file && matches(last) {
            return Some(last);
        }
        if let Some(at) = (start..=last).find(|&at| matches(at)) {
            return Some(at);
        }
    }
    None
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;
    use trex_store::library::Library;

    use super::*;
    use crate::{
        event::Event, sandbox::LazySandbox, test_support::sandbox_for_new_user, tool::Tools,
    };

    const SOURCE: &str = "def add(a, b):\n    return a + b\n\n\ndef sub(a, b):\n    return a + b\n";

    fn update(patch: &str) -> anyhow::Result<String> {
        let mut hunks = parse(patch)?;
        let Hunk::Update { chunks, .. } = hunks.remove(0) else {
            panic!("expected an update");
        };
        apply_chunks("calc.py", SOURCE, &chunks)
    }

    #[test]
    fn parses_every_kind_of_hunk() {
        let patch = "*** Begin Patch\n*** Add File: new.txt\n+hello\n+world\n*** Delete File: old.txt\n\
            *** Update File: a.py\n*** Move to: b.py\n@@ def f():\n-    x\n+    y\n*** End Patch";
        let hunks = parse(patch).unwrap();
        assert_eq!(
            hunks,
            [
                Hunk::Add {
                    path: "new.txt".into(),
                    content: "hello\nworld\n".into()
                },
                Hunk::Delete {
                    path: "old.txt".into()
                },
                Hunk::Update {
                    path: "a.py".into(),
                    move_to: Some("b.py".into()),
                    chunks: vec![Chunk {
                        anchor: Some("def f():".into()),
                        old: vec!["    x".into()],
                        new: vec!["    y".into()],
                        end_of_file: false,
                    }],
                },
            ]
        );
    }

    #[test]
    fn rejects_malformed_patches() {
        assert!(parse("*** Add File: x\n+y\n*** End Patch").is_err());
        assert!(parse("*** Begin Patch\n*** End Patch").is_err());
        assert!(parse("*** Begin Patch\n*** Add File: x\nno plus\n*** End Patch").is_err());
        assert!(parse("*** Begin Patch\n*** Update File: x\n*** End Patch").is_err());
        assert!(parse("*** Begin Patch\nhello\n*** End Patch").is_err());
    }

    #[test]
    fn uses_the_anchor_to_pick_the_right_place() {
        let fixed = update(
            "*** Begin Patch\n*** Update File: calc.py\n@@ def sub(a, b):\n-    return a + b\n+    return a - b\n*** End Patch",
        )
        .unwrap();
        assert_eq!(
            fixed,
            "def add(a, b):\n    return a + b\n\n\ndef sub(a, b):\n    return a - b\n"
        );
    }

    #[test]
    fn applies_several_chunks_and_tolerates_whitespace() {
        let fixed = update(
            "*** Begin Patch\n*** Update File: calc.py\n@@\n def add(a, b):\n-    return a + b   \n+    return b + a\n\
             @@ def sub(a, b):\n-  return a + b\n+    return a - b\n*** End Patch",
        )
        .unwrap();
        assert_eq!(
            fixed,
            "def add(a, b):\n    return b + a\n\n\ndef sub(a, b):\n    return a - b\n"
        );
    }

    #[test]
    fn appends_pure_additions_at_the_end() {
        let fixed =
            update("*** Begin Patch\n*** Update File: calc.py\n@@\n+\n+PI = 3.14\n*** End Patch")
                .unwrap();
        assert!(
            fixed.ends_with("    return a + b\n\nPI = 3.14\n"),
            "{fixed}"
        );
    }

    #[test]
    fn explains_lines_it_cannot_find() {
        let error = update(
            "*** Begin Patch\n*** Update File: calc.py\n@@\n-    return a * b\n+    return 0\n*** End Patch",
        )
        .unwrap_err()
        .to_string();
        assert!(
            error.contains("could not find these lines in calc.py"),
            "{error}"
        );
        assert!(error.contains("return a * b"), "{error}");
    }

    #[test]
    fn resolves_relative_paths_in_the_workdir() {
        assert_eq!(resolve("src/main.rs"), "/sandbox/src/main.rs");
        assert_eq!(resolve("/tmp/x"), "/tmp/x");
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and the dev image
    #[tokio::test]
    #[ignore]
    async fn patches_files_in_the_sandbox() {
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let tools = Tools::standard().unwrap();
        let (events, mut rx) = mpsc::channel(64);
        let call = |patch: &str| {
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
            let args = json!({ "patch": patch }).to_string();
            async move { tools.call(ctx, "apply_patch", &args).await }
        };
        let (shell, sandbox_ref) = (&openshell, &sandbox);
        let cat = |path: &'static str| async move {
            let output = shell
                .output(
                    sandbox_ref.get_if_ready().unwrap(),
                    [
                        "sh",
                        "-c",
                        &format!("cat {path} 2>/dev/null || echo MISSING"),
                    ]
                    .map(String::from)
                    .to_vec(),
                    Vec::new(),
                )
                .await
                .unwrap();
            String::from_utf8(output.stdout).unwrap()
        };

        let created = call(
            "*** Begin Patch\n*** Add File: proj/calc.py\n+def sub(a, b):\n+    return a + b\n\
             *** Add File: proj/notes.txt\n+todo\n*** End Patch",
        )
        .await;
        let failed = call(
            "*** Begin Patch\n*** Update File: proj/notes.txt\n-todo\n+done\n\
             *** Update File: proj/calc.py\n-    return a * b\n+    return 0\n*** End Patch",
        )
        .await;
        let notes_after_failure = cat("/sandbox/proj/notes.txt").await;
        let changed = call(
            "*** Begin Patch\n*** Update File: proj/calc.py\n*** Move to: proj/math.py\n@@ def sub(a, b):\n\
             -    return a + b\n+    return a - b\n*** Delete File: proj/notes.txt\n*** End Patch",
        )
        .await;
        let (moved, old, notes) = (
            cat("/sandbox/proj/math.py").await,
            cat("/sandbox/proj/calc.py").await,
            cat("/sandbox/proj/notes.txt").await,
        );
        openshell.delete_workspace(user).await.unwrap();

        let mut changes = Vec::new();
        while let Ok(event) = rx.try_recv() {
            if let Event::FileChanged { path, diff, .. } = event {
                changes.push((path, diff));
            }
        }
        assert_eq!(
            created.unwrap(),
            "A /sandbox/proj/calc.py\nA /sandbox/proj/notes.txt"
        );
        let failed = failed.unwrap_err().to_string();
        assert!(failed.contains("could not find these lines"), "{failed}");
        assert_eq!(
            notes_after_failure, "todo\n",
            "a failed patch changes nothing"
        );
        assert_eq!(
            changed.unwrap(),
            "R /sandbox/proj/calc.py -> /sandbox/proj/math.py\nD /sandbox/proj/notes.txt"
        );
        assert_eq!(moved, "def sub(a, b):\n    return a - b\n");
        assert_eq!((old.as_str(), notes.as_str()), ("MISSING\n", "MISSING\n"));
        assert_eq!(changes.len(), 4);
        let (path, diff) = &changes[2];
        assert_eq!(path, "/sandbox/proj/math.py");
        assert!(
            diff.contains("--- a/sandbox/proj/calc.py") && diff.contains("+    return a - b"),
            "{diff}"
        );
    }
}
