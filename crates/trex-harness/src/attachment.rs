use std::collections::HashMap;

use anyhow::{Context, bail};
use async_openai::types::responses::{
    EasyInputContent, EasyInputMessage, ImageDetail, InputContent, InputFileContent,
    InputImageContent, InputItem, InputTextContent, MessageType, Role,
};
use base64::{Engine, engine::general_purpose::STANDARD};
use serde_json::Value;
use trex_store::library::Library;
use uuid::Uuid;

const SCHEME: &str = "attachment://";
// fields of input_image and input_file parts that carry their content
const CONTENT_FIELDS: [&str; 2] = ["image_url", "file_data"];
pub const MAX_ATTACHMENT_BYTES: usize = 20 * 1024 * 1024;
const UNKNOWN_MIME: &str = "application/octet-stream";

// what history stores in place of the bytes, which would otherwise be resent and saved every turn
pub fn reference(hash: &str, mime: &str) -> String {
    format!("{SCHEME}{hash}#{mime}")
}

pub fn image_mime(bytes: &[u8]) -> Option<&'static str> {
    if bytes.starts_with(b"\x89PNG\r\n\x1a\n") {
        Some("image/png")
    } else if bytes.starts_with(b"\xff\xd8\xff") {
        Some("image/jpeg")
    } else if bytes.starts_with(b"GIF87a") || bytes.starts_with(b"GIF89a") {
        Some("image/gif")
    } else if bytes.len() > 12 && &bytes[..4] == b"RIFF" && &bytes[8..12] == b"WEBP" {
        Some("image/webp")
    } else {
        None
    }
}

// stores a file the user attached and returns the content part that refers to it; the type comes
// from the bytes, since a client's claimed mime type can't be trusted
pub async fn store(
    library: &Library,
    user: Uuid,
    filename: Option<&str>,
    bytes: Vec<u8>,
) -> anyhow::Result<InputContent> {
    if bytes.is_empty() || bytes.len() > MAX_ATTACHMENT_BYTES {
        bail!("an attachment must be 1 to {MAX_ATTACHMENT_BYTES} bytes");
    }
    let mime = sniff(&bytes);
    let image = image_mime(&bytes).is_some();
    if mime == UNKNOWN_MIME {
        bail!("attachments can be PNG, JPEG, GIF or WebP images, PDFs, or text files");
    }
    let hash = library.put_attachment(user, bytes).await?;
    let url = reference(&hash, mime);
    Ok(if image {
        InputContent::InputImage(InputImageContent {
            detail: ImageDetail::Auto,
            file_id: None,
            image_url: Some(url),
            prompt_cache_breakpoint: None,
        })
    } else {
        InputContent::InputFile(InputFileContent {
            file_data: Some(url),
            file_id: None,
            file_url: None,
            filename: Some(filename.unwrap_or("attachment").to_owned()),
            detail: None,
            prompt_cache_breakpoint: None,
        })
    })
}

pub fn sniff(bytes: &[u8]) -> &'static str {
    if let Some(mime) = image_mime(bytes) {
        mime
    } else if bytes.starts_with(b"%PDF-") {
        "application/pdf"
    } else if std::str::from_utf8(bytes).is_ok() {
        "text/plain; charset=utf-8"
    } else {
        UNKNOWN_MIME
    }
}

pub fn decode_data_url(url: &str) -> anyhow::Result<Vec<u8>> {
    let (header, data) = url
        .strip_prefix("data:")
        .and_then(|rest| rest.split_once(','))
        .context("expected a data url like data:image/png;base64,...")?;
    if !header.ends_with(";base64") {
        bail!("data urls must be base64 encoded");
    }
    STANDARD.decode(data).context("invalid base64 in data url")
}

pub fn user_message(text: &str, attachments: Vec<InputContent>) -> InputItem {
    if attachments.is_empty() {
        return EasyInputMessage::from(text).into();
    }
    let mut parts = vec![InputContent::InputText(InputTextContent {
        text: text.to_owned(),
        prompt_cache_breakpoint: None,
    })];
    parts.extend(attachments);
    InputItem::EasyMessage(EasyInputMessage {
        r#type: MessageType::Message,
        role: Role::User,
        content: EasyInputContent::ContentList(parts),
        phase: None,
    })
}

// the attachments of a stored item, for showing it: (kind, hash, mime, filename)
pub fn references(item: &Value) -> Vec<(&'static str, String, String, Option<String>)> {
    let Some(parts) = item["content"].as_array() else {
        return Vec::new();
    };
    parts
        .iter()
        .filter_map(|part| {
            let (kind, url) = match part["type"].as_str()? {
                "input_image" => ("image", part["image_url"].as_str()?),
                "input_file" => ("file", part["file_data"].as_str()?),
                _ => return None,
            };
            let (hash, mime) = url.strip_prefix(SCHEME)?.split_once('#')?;
            let filename = part["filename"].as_str().map(str::to_owned);
            Some((kind, hash.to_owned(), mime.to_owned(), filename))
        })
        .collect()
}

// swaps attachment references for data urls right before a request
pub async fn resolve(
    library: &Library,
    user: Uuid,
    items: Vec<InputItem>,
) -> anyhow::Result<Vec<InputItem>> {
    let mut value = serde_json::to_value(&items).context("failed to serialize the request")?;
    let mut references = Vec::new();
    visit(&mut value, &mut |field| references.push(field.clone()));
    if references.is_empty() {
        return Ok(items);
    }

    let mut data_urls = HashMap::new();
    for reference in references {
        if data_urls.contains_key(&reference) {
            continue;
        }
        let (hash, mime) = reference[SCHEME.len()..]
            .split_once('#')
            .with_context(|| format!("invalid attachment reference {reference}"))?;
        let bytes = library.get_attachment(user, hash).await?;
        let data_url = format!("data:{mime};base64,{}", STANDARD.encode(bytes));
        data_urls.insert(reference, data_url);
    }
    visit(&mut value, &mut |field| {
        *field = data_urls[field.as_str()].clone()
    });
    serde_json::from_value(value).context("failed to rebuild the request")
}

fn visit(value: &mut Value, apply: &mut impl FnMut(&mut String)) {
    match value {
        Value::Object(fields) => {
            for (name, field) in fields.iter_mut() {
                match field {
                    Value::String(text)
                        if CONTENT_FIELDS.contains(&name.as_str()) && text.starts_with(SCHEME) =>
                    {
                        apply(text)
                    }
                    _ => visit(field, apply),
                }
            }
        }
        Value::Array(items) => items.iter_mut().for_each(|item| visit(item, apply)),
        _ => {}
    }
}

#[cfg(test)]
mod tests {
    use serde_json::json;

    use super::*;

    #[tokio::test]
    async fn inlines_attachments_into_requests() {
        let library = Library::in_memory();
        let user = Uuid::now_v7();
        let hash = library
            .put_attachment(user, b"png!".to_vec())
            .await
            .unwrap();
        let reference = reference(&hash, "image/png");
        let items: Vec<InputItem> = serde_json::from_value(json!([
            {"role": "user", "content": [
                {"type": "input_text", "text": "what is this?"},
                {"type": "input_image", "image_url": reference, "detail": "auto"}
            ]},
            {"type": "function_call_output", "call_id": "c1", "output": [
                {"type": "input_image", "image_url": reference, "detail": "auto"}
            ]},
            {"role": "user", "content": "text mentioning attachment://not-a-field"}
        ]))
        .unwrap();

        let resolved = serde_json::to_value(resolve(&library, user, items).await.unwrap()).unwrap();
        let data_url = "data:image/png;base64,cG5nIQ==";
        assert_eq!(resolved[0]["content"][1]["image_url"], data_url);
        assert_eq!(resolved[1]["output"][0]["image_url"], data_url);
        assert_eq!(
            resolved[2]["content"],
            "text mentioning attachment://not-a-field"
        );
    }

    #[tokio::test]
    async fn stores_attachments_by_their_real_type() {
        let library = Library::in_memory();
        let user = Uuid::now_v7();
        let png = decode_data_url("data:image/png;base64,iVBORw0KGgoAAAA=").unwrap();
        let image = store(&library, user, Some("lies.pdf"), png).await.unwrap();
        let pdf = store(&library, user, None, b"%PDF-1.7 ...".to_vec())
            .await
            .unwrap();
        let text = store(&library, user, Some("notes.md"), b"# notes".to_vec())
            .await
            .unwrap();
        assert!(
            store(&library, user, None, vec![0, 159, 146, 150])
                .await
                .is_err()
        );
        assert!(store(&library, user, None, Vec::new()).await.is_err());
        assert!(decode_data_url("https://example.com/a.png").is_err());

        let message = serde_json::to_value(user_message("look", vec![image, pdf, text])).unwrap();
        assert_eq!(message["content"][0]["text"], "look");
        let kinds: Vec<_> = references(&message)
            .into_iter()
            .map(|(kind, _, mime, filename)| (kind, mime, filename))
            .collect();
        assert_eq!(
            kinds,
            [
                ("image", "image/png".to_owned(), None),
                (
                    "file",
                    "application/pdf".to_owned(),
                    Some("attachment".to_owned())
                ),
                (
                    "file",
                    "text/plain; charset=utf-8".to_owned(),
                    Some("notes.md".to_owned())
                ),
            ]
        );
    }

    #[test]
    fn recognizes_images_by_their_bytes() {
        assert_eq!(image_mime(b"\x89PNG\r\n\x1a\nrest"), Some("image/png"));
        assert_eq!(image_mime(b"\xff\xd8\xff\xe0"), Some("image/jpeg"));
        assert_eq!(image_mime(b"RIFF\0\0\0\0WEBPVP8 "), Some("image/webp"));
        assert_eq!(image_mime(b"%PDF-1.7"), None);
    }
}
