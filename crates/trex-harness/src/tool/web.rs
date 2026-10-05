use std::{
    net::{IpAddr, Ipv4Addr, Ipv6Addr, SocketAddr},
    sync::Arc,
    time::Duration,
};

use anyhow::{Context, bail};
use async_openai::types::responses::FunctionTool;
use futures::future::BoxFuture;
use reqwest::{
    Url,
    dns::{Addrs, Name, Resolve, Resolving},
    header::CONTENT_TYPE,
    redirect,
};
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext, file::write};

const FETCH_TIMEOUT: Duration = Duration::from_secs(30);
const MAX_BODY_BYTES: usize = 25 * 1024 * 1024;
const MAX_REDIRECTS: usize = 5;
const PAGE_CHARS: usize = 30_000;

// fetches run on the trex host, not in the sandbox, so every address is checked to keep
// model-chosen urls from reaching internal services or cloud metadata endpoints
pub struct WebFetch {
    client: reqwest::Client,
}

#[derive(Deserialize)]
struct Args {
    url: String,
    offset: Option<usize>,
    save_to: Option<String>,
}

impl WebFetch {
    pub fn new() -> anyhow::Result<Self> {
        let client = reqwest::Client::builder()
            .user_agent(concat!("trex/", env!("CARGO_PKG_VERSION")))
            .timeout(FETCH_TIMEOUT)
            .no_proxy()
            .dns_resolver(Arc::new(PublicOnly))
            .redirect(redirect::Policy::custom(|attempt| {
                if attempt.previous().len() >= MAX_REDIRECTS {
                    attempt.error(format!("more than {MAX_REDIRECTS} redirects"))
                } else if let Err(error) = check_url(attempt.url()) {
                    attempt.error(error.to_string())
                } else {
                    attempt.follow()
                }
            }))
            .build()
            .context("failed to build web client")?;
        Ok(Self { client })
    }

    async fn fetch(&self, url: &str) -> anyhow::Result<(Url, String, Vec<u8>)> {
        let url = Url::parse(url).context("invalid url")?;
        check_url(&url)?;

        let mut response = self
            .client
            .get(url)
            .send()
            .await
            .map_err(|error| anyhow::anyhow!("request failed: {}", error_chain(&error)))?;
        let status = response.status();
        if !status.is_success() {
            bail!("server returned {status}");
        }
        if response
            .content_length()
            .is_some_and(|len| len > MAX_BODY_BYTES as u64)
        {
            bail!("response is larger than {MAX_BODY_BYTES} bytes");
        }

        let final_url = response.url().clone();
        let content_type = response
            .headers()
            .get(CONTENT_TYPE)
            .and_then(|value| value.to_str().ok())
            .unwrap_or_default()
            .to_ascii_lowercase();

        let mut body = Vec::new();
        while let Some(chunk) = response.chunk().await.context("failed to read response")? {
            if body.len() + chunk.len() > MAX_BODY_BYTES {
                bail!("response is larger than {MAX_BODY_BYTES} bytes");
            }
            body.extend_from_slice(&chunk);
        }
        Ok((final_url, content_type, body))
    }
}

impl Tool for WebFetch {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "web_fetch".into(),
            description: Some(format!(
                "Fetch a public http(s) url. HTML is returned as markdown, PDFs as extracted text, and text or JSON as-is, \
                 {PAGE_CHARS} characters at a time; pass offset to read further. Set save_to to download the raw file \
                 (any type, up to {MAX_BODY_BYTES} bytes) into the sandbox instead of reading it."
            )),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "url": {"type": "string", "description": "The http or https url to fetch."},
                    "offset": {"type": ["integer", "null"], "description": "Character offset into the converted text, for reading long documents."},
                    "save_to": {"type": ["string", "null"], "description": "Sandbox path to save the raw response to."}
                },
                "required": ["url", "offset", "save_to"],
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
            let (url, content_type, body) = self.fetch(&args.url).await?;

            if let Some(path) = args.save_to {
                write(&ctx, &path, &body).await?;
                return Ok(format!(
                    "saved {url} ({content_type}, {} bytes) to {path}",
                    body.len()
                ));
            }

            let title = content_type
                .contains("html")
                .then(|| html_title(&String::from_utf8_lossy(&body)))
                .flatten()
                .map(|title| format!("title: {title}\n"))
                .unwrap_or_default();
            let text = to_text(&content_type, body).await?;
            Ok(format!(
                "url: {url}\ncontent-type: {content_type}\n{title}\n{}",
                page(&text, args.offset.unwrap_or(0))
            ))
        })
    }
}

struct PublicOnly;

impl Resolve for PublicOnly {
    fn resolve(&self, name: Name) -> Resolving {
        Box::pin(async move {
            let host = name.as_str().to_owned();
            let addrs: Vec<SocketAddr> = tokio::net::lookup_host((host.as_str(), 0))
                .await?
                .filter(|addr| is_public(addr.ip()))
                .collect();
            if addrs.is_empty() {
                return Err(format!("{host} does not resolve to a public address").into());
            }
            Ok(Box::new(addrs.into_iter()) as Addrs)
        })
    }
}

// the resolver never sees literal ip hosts, so those are checked here, including on redirects
fn check_url(url: &Url) -> anyhow::Result<()> {
    if !matches!(url.scheme(), "http" | "https") {
        bail!("only http and https urls can be fetched");
    }
    match url.host() {
        Some(url::Host::Ipv4(ip)) if !is_public(IpAddr::V4(ip)) => {
            bail!("{ip} is not a public address")
        }
        Some(url::Host::Ipv6(ip)) if !is_public(IpAddr::V6(ip)) => {
            bail!("{ip} is not a public address")
        }
        None => bail!("url has no host"),
        _ => Ok(()),
    }
}

fn is_public(ip: IpAddr) -> bool {
    match ip {
        IpAddr::V4(ip) => is_public_v4(ip),
        IpAddr::V6(ip) => match ip.to_ipv4_mapped() {
            Some(v4) => is_public_v4(v4),
            None => is_public_v6(ip),
        },
    }
}

fn is_public_v4(ip: Ipv4Addr) -> bool {
    let [a, b, c, _] = ip.octets();
    !(ip.is_private()
        || ip.is_loopback()
        || ip.is_link_local()
        || ip.is_unspecified()
        || ip.is_broadcast()
        || ip.is_documentation()
        || ip.is_multicast()
        || a == 0
        || (a == 100 && (64..128).contains(&b))
        || (a == 192 && b == 0 && c == 0)
        || (a == 198 && (b == 18 || b == 19))
        || a >= 240)
}

fn is_public_v6(ip: Ipv6Addr) -> bool {
    let segments = ip.segments();
    !(ip.is_loopback()
        || ip.is_unspecified()
        || ip.is_multicast()
        || ip.is_unique_local()
        || ip.is_unicast_link_local()
        || (segments[0] == 0x2001 && segments[1] == 0x0db8)
        // nat64 prefixes can translate to private ipv4 addresses
        || (segments[0] == 0x0064 && segments[1] == 0xff9b))
}

// the markdown conversion drops <head>, and the title is often the best summary of a page
fn html_title(html: &str) -> Option<String> {
    let lower = html.to_ascii_lowercase();
    let open = lower.find("<title")?;
    let start = open + lower[open..].find('>')? + 1;
    let end = start + lower[start..].find("</title")?;
    let title = html[start..end]
        .split_whitespace()
        .collect::<Vec<_>>()
        .join(" ");
    let title = title
        .replace("&amp;", "&")
        .replace("&lt;", "<")
        .replace("&gt;", ">")
        .replace("&quot;", "\"")
        .replace("&#39;", "'");
    (!title.is_empty()).then_some(title)
}

async fn to_text(content_type: &str, body: Vec<u8>) -> anyhow::Result<String> {
    let is_pdf = content_type.starts_with("application/pdf") || body.starts_with(b"%PDF-");
    if is_pdf {
        // pdf parsing is cpu heavy and can panic on malformed files, so it runs on a blocking thread
        return tokio::task::spawn_blocking(move || pdf_extract::extract_text_from_mem(&body))
            .await
            .map_err(|_| anyhow::anyhow!("failed to parse pdf"))?
            .map_err(|error| anyhow::anyhow!("failed to extract pdf text: {error}"));
    }

    let text = String::from_utf8_lossy(&body);
    if content_type.contains("html") {
        let converter = htmd::HtmlToMarkdown::builder()
            .skip_tags(vec!["script", "style", "noscript", "svg", "head", "iframe"])
            .build();
        return converter
            .convert(&text)
            .context("failed to convert html to markdown");
    }
    let textual = content_type.is_empty()
        || content_type.starts_with("text/")
        || content_type.contains("json")
        || content_type.contains("xml")
        || content_type.contains("javascript");
    if !textual {
        bail!(
            "{content_type} is not readable as text; use save_to to download it into the sandbox"
        );
    }
    Ok(text.into_owned())
}

fn page(text: &str, offset: usize) -> String {
    let chars: Vec<char> = text.chars().collect();
    let total = chars.len();
    if offset >= total && total > 0 {
        return format!("[offset {offset} is past the end; the document has {total} characters]");
    }
    let end = (offset + PAGE_CHARS).min(total);
    let mut out: String = chars[offset..end].iter().collect();
    if end < total {
        out.push_str(&format!(
            "\n\n[showing characters {offset}-{end} of {total}; call again with offset={end} to continue]"
        ));
    }
    out
}

fn error_chain(error: &dyn std::error::Error) -> String {
    let mut message = error.to_string();
    let mut source = error.source();
    while let Some(cause) = source {
        message.push_str(&format!(": {cause}"));
        source = cause.source();
    }
    message
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::sandbox::LazySandbox;

    #[test]
    fn finds_the_page_title() {
        let html = "<html><HEAD><Title lang=en>\n  Fish &amp; Chips\n</TITLE></head><body><h1>x</h1></body></html>";
        assert_eq!(html_title(html).as_deref(), Some("Fish & Chips"));
        assert_eq!(html_title("<p>no title</p>"), None);
        assert_eq!(html_title("<title>  </title>"), None);
    }

    #[test]
    fn classifies_addresses() {
        for ip in [
            "127.0.0.1",
            "10.1.2.3",
            "172.16.0.1",
            "192.168.0.112",
            "169.254.169.254",
            "100.64.0.1",
            "0.0.0.0",
            "::1",
            "fd00::1",
            "fe80::1",
            "::ffff:127.0.0.1",
            "64:ff9b::a00:1",
        ] {
            assert!(!is_public(ip.parse().unwrap()), "{ip} must be blocked");
        }
        for ip in ["93.184.215.14", "1.1.1.1", "2606:4700:4700::1111"] {
            assert!(is_public(ip.parse().unwrap()), "{ip} must be allowed");
        }
    }

    #[tokio::test]
    async fn rejects_internal_urls_before_connecting() {
        let fetch = WebFetch::new().unwrap();
        for url in [
            "http://127.0.0.1:8080/",
            "http://[::1]/",
            "http://169.254.169.254/latest/meta-data/",
            "http://localhost/",
            "file:///etc/passwd",
            "ftp://example.com/",
        ] {
            assert!(fetch.fetch(url).await.is_err(), "{url} must be rejected");
        }
    }

    #[test]
    fn pages_long_text() {
        let text = "a".repeat(PAGE_CHARS + 5);
        let first = page(&text, 0);
        assert!(first.ends_with(&format!(
            "[showing characters 0-{PAGE_CHARS} of {}; call again with offset={PAGE_CHARS} to continue]",
            PAGE_CHARS + 5
        )));
        assert_eq!(page(&text, PAGE_CHARS), "aaaaa");
        assert!(page(&text, PAGE_CHARS + 10).contains("past the end"));
    }

    #[tokio::test]
    async fn converts_html_and_rejects_binary() {
        let html = b"<html><head><title>t</title><script>evil()</script></head><body><h1>Hi</h1><p>there</p></body></html>";
        let markdown = to_text("text/html; charset=utf-8", html.to_vec())
            .await
            .unwrap();
        assert!(markdown.contains("# Hi"));
        assert!(!markdown.contains("evil"));
        assert!(to_text("image/png", vec![0x89, b'P']).await.is_err());
    }

    // needs internet access, the openshell gateway tunnel, certs, and the dev image
    #[tokio::test]
    #[ignore]
    async fn fetches_pages_and_pdfs() {
        use tokio::sync::mpsc;
        use trex_store::library::Library;

        use crate::{test_support::sandbox_for_new_user, tool::Tools};

        const PDF: &str = "https://www.w3.org/WAI/ER/tests/xhtml/testfiles/resources/pdf/dummy.pdf";

        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let (events, _rx) = mpsc::channel(16);
        let tools = Tools::standard().unwrap();
        let call = |args: Value| {
            let ctx = ToolContext {
                workspace: user,
                library: &library,
                openshell: &openshell,
                sandbox: &sandbox,
                call_id: "call_test",
                events: &events,
            };
            let tools = &tools;
            async move { tools.call(ctx, "web_fetch", &args.to_string()).await }
        };

        let html =
            call(json!({"url": "https://example.com", "offset": null, "save_to": null})).await;
        let pdf = call(json!({"url": PDF, "offset": null, "save_to": null})).await;
        let saved =
            call(json!({"url": PDF, "offset": null, "save_to": "/sandbox/dl/dummy.pdf"})).await;
        let header = openshell
            .output(
                sandbox.get_if_ready().unwrap(),
                ["head", "-c", "5", "/sandbox/dl/dummy.pdf"]
                    .map(String::from)
                    .to_vec(),
                Vec::new(),
            )
            .await
            .unwrap();
        let rebound =
            call(json!({"url": "http://localtest.me/", "offset": null, "save_to": null})).await;

        openshell.delete_workspace(user).await.unwrap();

        let html = html.unwrap();
        assert!(html.contains("content-type: text/html"), "{html}");
        assert!(html.contains("title: Example Domain"), "{html}");
        assert!(html.contains("documentation examples"), "{html}");
        assert!(pdf.unwrap().contains("Dummy PDF file"));
        assert!(
            saved
                .unwrap()
                .starts_with(&format!("saved {PDF} (application/pdf"))
        );
        assert_eq!(header.stdout, b"%PDF-");
        let rebound = rebound.unwrap_err().to_string();
        assert!(rebound.contains("public address"), "{rebound}");
    }
}
