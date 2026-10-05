use anyhow::bail;
use async_openai::types::responses::{
    FunctionTool, ImageDetail, InputContent, InputImageContent, InputTextContent,
};
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext, ToolOutput, file::read_bytes};
use crate::attachment::{self, MAX_ATTACHMENT_BYTES};

pub struct ViewImage;

#[derive(Deserialize)]
struct Args {
    path: String,
}

impl Tool for ViewImage {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "view_image".into(),
            description: Some(
                "Look at a PNG, JPEG, GIF or WebP image in the sandbox, for example a chart or screenshot you made, \
                 to check what it shows."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "path": {"type": "string", "description": "Path of the image in the sandbox."}
                },
                "required": ["path"],
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
            let bytes = read_bytes(&ctx, &args.path).await?;
            if bytes.len() > MAX_ATTACHMENT_BYTES {
                bail!(
                    "{} is {} bytes; images up to {MAX_ATTACHMENT_BYTES} bytes can be viewed",
                    args.path,
                    bytes.len()
                );
            }
            let Some(mime) = attachment::image_mime(&bytes) else {
                bail!("{} is not a PNG, JPEG, GIF or WebP image", args.path);
            };
            let size = bytes.len();
            let hash = ctx.library.put_attachment(ctx.workspace, bytes).await?;
            Ok(ToolOutput::Content(vec![
                InputContent::InputText(InputTextContent {
                    text: format!("{} ({mime}, {size} bytes):", args.path),
                    prompt_cache_breakpoint: None,
                }),
                InputContent::InputImage(InputImageContent {
                    detail: ImageDetail::Auto,
                    file_id: None,
                    image_url: Some(attachment::reference(&hash, mime)),
                    prompt_cache_breakpoint: None,
                }),
            ]))
        })
    }
}
