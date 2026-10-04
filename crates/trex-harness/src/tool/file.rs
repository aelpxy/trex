use anyhow::bail;
use async_openai::types::responses::FunctionTool;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext, truncate};

const DEFAULT_READ_LINES: usize = 2000;
const MAX_LINE_CHARS: usize = 2000;

pub struct ReadFile;
pub struct WriteFile;
pub struct EditFile;

#[derive(Deserialize)]
struct ReadArgs {
    path: String,
    offset: Option<usize>,
    limit: Option<usize>,
}

#[derive(Deserialize)]
struct WriteArgs {
    path: String,
    content: String,
}

#[derive(Deserialize)]
struct EditArgs {
    path: String,
    old_string: String,
    new_string: String,
    replace_all: Option<bool>,
}

impl Tool for ReadFile {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "read_file".into(),
            description: Some(format!(
                "Read a text file from the sandbox. Returns lines prefixed with their 1-based line number and a tab. \
                 Reads up to {DEFAULT_READ_LINES} lines by default; use offset and limit for large files."
            )),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute path of the file."},
                    "offset": {"type": ["integer", "null"], "description": "1-based line to start from."},
                    "limit": {"type": ["integer", "null"], "description": "Maximum number of lines to return."}
                },
                "required": ["path", "offset", "limit"],
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
            let args: ReadArgs = serde_json::from_value(args)?;
            let content = read(&ctx, &args.path).await?;
            let offset = args.offset.unwrap_or(1).max(1);
            let limit = args.limit.unwrap_or(DEFAULT_READ_LINES);
            Ok(truncate(&number_lines(&content, offset, limit)))
        })
    }
}

impl Tool for WriteFile {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "write_file".into(),
            description: Some(
                "Create or overwrite a file in the sandbox with the given content. Parent directories are created. \
                 Prefer edit_file for changing part of an existing file."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute path of the file."},
                    "content": {"type": "string", "description": "The full file content."}
                },
                "required": ["path", "content"],
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
            let args: WriteArgs = serde_json::from_value(args)?;
            write(&ctx, &args.path, args.content.as_bytes()).await?;
            Ok(format!(
                "wrote {} bytes to {}",
                args.content.len(),
                args.path
            ))
        })
    }
}

impl Tool for EditFile {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "edit_file".into(),
            description: Some(
                "Replace an exact string in a file in the sandbox. old_string must match the file exactly, \
                 including whitespace, and be unique unless replace_all is true. Read the file first."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Absolute path of the file."},
                    "old_string": {"type": "string", "description": "The exact text to replace."},
                    "new_string": {"type": "string", "description": "The replacement text."},
                    "replace_all": {"type": ["boolean", "null"], "description": "Replace every occurrence instead of exactly one."}
                },
                "required": ["path", "old_string", "new_string", "replace_all"],
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
            let args: EditArgs = serde_json::from_value(args)?;
            let content = read(&ctx, &args.path).await?;
            let (edited, count) = apply_edit(
                &content,
                &args.old_string,
                &args.new_string,
                args.replace_all.unwrap_or(false),
            )?;
            write(&ctx, &args.path, edited.as_bytes()).await?;
            Ok(format!(
                "replaced {count} occurrence{} in {}",
                if count == 1 { "" } else { "s" },
                args.path
            ))
        })
    }
}

async fn read(ctx: &ToolContext<'_>, path: &str) -> anyhow::Result<String> {
    match String::from_utf8(read_bytes(ctx, path).await?) {
        Ok(content) => Ok(content),
        Err(_) => bail!("{path} is not a utf-8 text file"),
    }
}

pub(super) async fn read_bytes(ctx: &ToolContext<'_>, path: &str) -> anyhow::Result<Vec<u8>> {
    let argv = ["cat", "--", path].map(String::from).to_vec();
    let output = ctx.openshell.output(ctx.sandbox, argv, Vec::new()).await?;
    if output.exit_code != Some(0) {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(output.stdout)
}

pub(super) async fn write(ctx: &ToolContext<'_>, path: &str, content: &[u8]) -> anyhow::Result<()> {
    // the path is passed as $1 so it is never interpreted by the shell
    let script = r#"mkdir -p -- "$(dirname -- "$1")" && cat > "$1""#;
    let argv = ["sh", "-c", script, "sh", path].map(String::from).to_vec();
    let output = ctx
        .openshell
        .output(ctx.sandbox, argv, content.to_vec())
        .await?;
    if output.exit_code != Some(0) {
        bail!("{}", String::from_utf8_lossy(&output.stderr).trim());
    }
    Ok(())
}

fn number_lines(content: &str, offset: usize, limit: usize) -> String {
    if content.is_empty() {
        return "[empty file]".into();
    }

    let total = content.lines().count();
    if offset > total {
        return format!("[offset {offset} is past the end of the file, which has {total} lines]");
    }

    let mut out = String::new();
    let mut last = offset;
    for (index, line) in content.lines().enumerate().skip(offset - 1).take(limit) {
        last = index + 1;
        let line = match line.char_indices().nth(MAX_LINE_CHARS) {
            Some((cut, _)) => format!("{}[... line truncated]", &line[..cut]),
            None => line.to_owned(),
        };
        out.push_str(&format!("{last:>6}\t{line}\n"));
    }

    if last < total {
        out.push_str(&format!(
            "[showing lines {offset}-{last} of {total}; use offset to read more]\n"
        ));
    }
    out
}

fn apply_edit(
    content: &str,
    old: &str,
    new: &str,
    replace_all: bool,
) -> anyhow::Result<(String, usize)> {
    if old.is_empty() {
        bail!("old_string must not be empty");
    }
    if old == new {
        bail!("old_string and new_string are identical");
    }

    let count = content.matches(old).count();
    match count {
        0 => bail!("old_string was not found in the file"),
        1 => Ok((content.replacen(old, new, 1), 1)),
        _ if replace_all => Ok((content.replace(old, new), count)),
        _ => bail!(
            "old_string appears {count} times; add surrounding context to make it unique or set replace_all"
        ),
    }
}

#[cfg(test)]
mod tests {
    use tokio::sync::mpsc;
    use trex_store::library::Library;

    use super::*;
    use crate::{test_support::sandbox_for_new_user, tool::Tools};

    // needs the openshell gateway tunnel and <workspace>/certs/openshell: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn file_tools_round_trip_in_sandbox() {
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let (events, _rx) = mpsc::channel(64);
        let tools = Tools::standard();
        let library = Library::in_memory();
        let ctx = || ToolContext {
            user,
            library: &library,
            openshell: &openshell,
            sandbox: &sandbox,
            call_id: "call_test",
            events: &events,
        };
        let path = "/tmp/trex test/$(touch pwned).txt";
        let call = |name: &'static str, args: Value| {
            let ctx = ctx();
            let tools = &tools;
            async move { tools.call(ctx, name, &args.to_string()).await }
        };

        let wrote = call("write_file", json!({"path": path, "content": "one\ntwo\n"})).await;
        let read = call(
            "read_file",
            json!({"path": path, "offset": null, "limit": null}),
        )
        .await;
        let edited = call(
            "edit_file",
            json!({"path": path, "old_string": "two", "new_string": "three", "replace_all": null}),
        )
        .await;
        let reread = call("read_file", json!({"path": path, "offset": 2, "limit": 1})).await;
        let missing = call(
            "read_file",
            json!({"path": "/tmp/nope.txt", "offset": null, "limit": null}),
        )
        .await;
        let listing = openshell
            .output(
                &sandbox,
                ["sh", "-c", "ls -A /tmp/trex\\ test; ls -A ."]
                    .map(String::from)
                    .to_vec(),
                Vec::new(),
            )
            .await
            .unwrap();

        openshell.delete_workspace(user).await.unwrap();

        assert_eq!(wrote.unwrap(), format!("wrote 8 bytes to {path}"));
        assert_eq!(read.unwrap(), "     1\tone\n     2\ttwo\n");
        assert_eq!(edited.unwrap(), format!("replaced 1 occurrence in {path}"));
        assert_eq!(reread.unwrap(), "     2\tthree\n");
        assert!(missing.unwrap_err().to_string().contains("No such file"));
        let listing = String::from_utf8(listing.stdout).unwrap();
        assert!(listing.contains("$(touch pwned).txt"));
        assert!(!listing.lines().any(|line| line == "pwned"));
    }

    #[test]
    fn numbers_lines_from_offset() {
        let content = "a\nb\nc\nd\n";
        assert_eq!(
            number_lines(content, 2, 2),
            "     2\tb\n     3\tc\n[showing lines 2-3 of 4; use offset to read more]\n"
        );
        assert_eq!(number_lines(content, 3, 10), "     3\tc\n     4\td\n");
    }

    #[test]
    fn numbers_lines_edge_cases() {
        assert_eq!(number_lines("", 1, 10), "[empty file]");
        assert_eq!(
            number_lines("a\n", 5, 10),
            "[offset 5 is past the end of the file, which has 1 lines]"
        );
    }

    #[test]
    fn truncates_long_lines() {
        let line = "x".repeat(MAX_LINE_CHARS + 10);
        let numbered = number_lines(&line, 1, 1);
        assert!(numbered.contains("[... line truncated]"));
        assert_eq!(numbered.matches('x').count(), MAX_LINE_CHARS);
    }

    #[test]
    fn edits_unique_match() {
        let (edited, count) =
            apply_edit("let a = 1;\nlet b = 2;\n", "b = 2", "b = 3", false).unwrap();
        assert_eq!(edited, "let a = 1;\nlet b = 3;\n");
        assert_eq!(count, 1);
    }

    #[test]
    fn rejects_ambiguous_edit_unless_replace_all() {
        let content = "x\nx\n";
        let error = apply_edit(content, "x", "y", false).unwrap_err();
        assert!(error.to_string().contains("appears 2 times"));
        assert_eq!(
            apply_edit(content, "x", "y", true).unwrap(),
            ("y\ny\n".into(), 2)
        );
    }

    #[test]
    fn rejects_missing_and_noop_edits() {
        assert!(apply_edit("abc", "z", "y", false).is_err());
        assert!(apply_edit("abc", "b", "b", false).is_err());
        assert!(apply_edit("abc", "", "y", false).is_err());
    }
}
