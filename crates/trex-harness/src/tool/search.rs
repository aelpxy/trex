use anyhow::bail;
use async_openai::types::responses::FunctionTool;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext, truncate};

const DEFAULT_PATH: &str = "/sandbox";
const MAX_FILES: usize = 500;

pub struct Grep;
pub struct Glob;

#[derive(Deserialize)]
struct GrepArgs {
    pattern: String,
    path: Option<String>,
    glob: Option<String>,
    ignore_case: Option<bool>,
}

#[derive(Deserialize)]
struct GlobArgs {
    pattern: String,
    path: Option<String>,
}

impl Tool for Grep {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "grep".into(),
            description: Some(
                "Search file contents with ripgrep. Returns matching lines as path:line:text. \
                 Respects .gitignore. Prefer this over running grep or rg through bash."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Rust regex to search for."},
                    "path": {"type": ["string", "null"], "description": "File or directory to search; defaults to /sandbox."},
                    "glob": {"type": ["string", "null"], "description": "Only search files matching this glob, e.g. *.rs or src/**/*.ts."},
                    "ignore_case": {"type": ["boolean", "null"], "description": "Match case-insensitively."}
                },
                "required": ["pattern", "path", "glob", "ignore_case"],
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
            let args: GrepArgs = serde_json::from_value(args)?;
            let argv = grep_argv(&args);
            let output = ctx.openshell.output(ctx.sandbox, argv, Vec::new()).await?;
            // ripgrep exits 1 when nothing matched and 2 on errors
            match output.exit_code {
                Some(0) => Ok(truncate(&String::from_utf8_lossy(&output.stdout))),
                Some(1) => Ok("no matches".into()),
                _ => bail!("{}", String::from_utf8_lossy(&output.stderr).trim()),
            }
        })
    }
}

impl Tool for Glob {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "glob".into(),
            description: Some(format!(
                "Find files by glob pattern, e.g. **/*.rs or src/**/test_*.py, relative to path. \
                 Returns sorted paths, at most {MAX_FILES}. Respects .gitignore."
            )),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "pattern": {"type": "string", "description": "Glob pattern relative to path."},
                    "path": {"type": ["string", "null"], "description": "Directory to search; defaults to /sandbox."}
                },
                "required": ["pattern", "path"],
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
            let args: GlobArgs = serde_json::from_value(args)?;
            let path = args.path.as_deref().unwrap_or(DEFAULT_PATH);
            let argv = [
                "rg",
                "--files",
                "--hidden",
                "--sort",
                "path",
                "--glob",
                "!.git",
                "--glob",
                &args.pattern,
                "--",
                path,
            ]
            .map(String::from)
            .to_vec();
            let output = ctx.openshell.output(ctx.sandbox, argv, Vec::new()).await?;
            match output.exit_code {
                Some(0) => Ok(cap_lines(
                    &String::from_utf8_lossy(&output.stdout),
                    MAX_FILES,
                )),
                Some(1) => Ok("no files matched".into()),
                _ => bail!("{}", String::from_utf8_lossy(&output.stderr).trim()),
            }
        })
    }
}

fn grep_argv(args: &GrepArgs) -> Vec<String> {
    let mut argv: Vec<String> = [
        "rg",
        "--line-number",
        "--no-heading",
        "--with-filename",
        "--color=never",
        "--max-columns=500",
        "--max-columns-preview",
    ]
    .map(String::from)
    .to_vec();
    if args.ignore_case.unwrap_or(false) {
        argv.push("--ignore-case".into());
    }
    if let Some(glob) = &args.glob {
        argv.extend(["--glob".into(), glob.clone()]);
    }
    // -- keeps a pattern starting with a dash from being read as a flag
    argv.extend([
        "--".into(),
        args.pattern.clone(),
        args.path.clone().unwrap_or_else(|| DEFAULT_PATH.into()),
    ]);
    argv
}

fn cap_lines(output: &str, max: usize) -> String {
    let total = output.lines().count();
    let mut capped: String = output.lines().take(max).collect::<Vec<_>>().join("\n");
    if total > max {
        capped.push_str(&format!(
            "\n[showing {max} of {total} files; narrow the pattern]"
        ));
    }
    capped
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn grep_argv_guards_pattern_and_defaults_path() {
        let args = GrepArgs {
            pattern: "-rf".into(),
            path: None,
            glob: Some("*.rs".into()),
            ignore_case: Some(true),
        };
        let argv = grep_argv(&args);
        assert!(argv.contains(&"--ignore-case".to_owned()));
        assert_eq!(
            &argv[argv.len() - 5..],
            ["--glob", "*.rs", "--", "-rf", "/sandbox"]
        );
    }

    #[test]
    fn caps_long_file_lists() {
        let output = (0..5)
            .map(|i| format!("f{i}"))
            .collect::<Vec<_>>()
            .join("\n");
        assert_eq!(cap_lines(&output, 10), output);
        assert_eq!(
            cap_lines(&output, 2),
            "f0\nf1\n[showing 2 of 5 files; narrow the pattern]"
        );
    }
}
