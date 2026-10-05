use anyhow::bail;
use async_openai::types::responses::FunctionTool;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{
    Tool, ToolContext,
    file::{read_bytes, write},
};

const MAX_SAVE_BYTES: usize = 100 * 1024 * 1024;

pub struct LibraryList;
pub struct LibraryLoad;
pub struct LibrarySave;

#[derive(Deserialize)]
struct LoadArgs {
    library_path: String,
    sandbox_path: Option<String>,
}

#[derive(Deserialize)]
struct SaveArgs {
    sandbox_path: String,
    library_path: String,
}

impl Tool for LibraryList {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "library_list".into(),
            description: Some(
                "List the files in the user's personal library, which persists across conversations."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {},
                "required": [],
                "additionalProperties": false,
            })),
            strict: Some(true),
            ..Default::default()
        }
    }

    fn call<'a>(
        &'a self,
        ctx: ToolContext<'a>,
        _args: Value,
    ) -> BoxFuture<'a, anyhow::Result<String>> {
        Box::pin(async move {
            let files = ctx.library.list(ctx.workspace).await?;
            if files.is_empty() {
                return Ok("the library is empty".into());
            }
            let lines: Vec<_> = files
                .iter()
                .map(|file| format!("{} ({} bytes)", file.path, file.size))
                .collect();
            Ok(lines.join("\n"))
        })
    }
}

impl Tool for LibraryLoad {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "library_load".into(),
            description: Some(
                "Copy a file from the user's library into the sandbox so it can be read or processed."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "library_path": {"type": "string", "description": "Path in the library, as shown by library_list."},
                    "sandbox_path": {"type": ["string", "null"], "description": "Destination in the sandbox; defaults to /sandbox/library/<library_path>."}
                },
                "required": ["library_path", "sandbox_path"],
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
            let args: LoadArgs = serde_json::from_value(args)?;
            let content = ctx.library.get(ctx.workspace, &args.library_path).await?;
            let dest = args
                .sandbox_path
                .unwrap_or_else(|| format!("/sandbox/library/{}", args.library_path));
            write(&ctx, &dest, &content).await?;
            Ok(format!(
                "copied {} ({} bytes) to {dest}",
                args.library_path,
                content.len()
            ))
        })
    }
}

impl Tool for LibrarySave {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "library_save".into(),
            description: Some(
                "Save a file from the sandbox into the user's library so they can keep and download it. \
                 Use this for deliverables the user asked for. Overwrites an existing library file at the same path."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "sandbox_path": {"type": "string", "description": "File in the sandbox to save."},
                    "library_path": {"type": "string", "description": "Destination path in the library, e.g. reports/summary.md."}
                },
                "required": ["sandbox_path", "library_path"],
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
            let args: SaveArgs = serde_json::from_value(args)?;
            let content = read_bytes(&ctx, &args.sandbox_path).await?;
            if content.len() > MAX_SAVE_BYTES {
                bail!(
                    "{} is {} bytes; the library limit is {MAX_SAVE_BYTES} bytes per file",
                    args.sandbox_path,
                    content.len()
                );
            }
            let size = content.len();
            ctx.library
                .put(ctx.workspace, &args.library_path, content)
                .await?;
            Ok(format!(
                "saved {} ({size} bytes) to the library as {}",
                args.sandbox_path, args.library_path
            ))
        })
    }
}
