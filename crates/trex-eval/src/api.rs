use std::{
    pin::Pin,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::{Context, bail};
use eventsource_stream::{Event, EventStreamError, Eventsource};
use futures::{Stream, StreamExt};
use reqwest::{Client, Method, StatusCode};
use serde_json::{Value, json};
use tokio::time::sleep;
use uuid::Uuid;

const RUN_ENDS: [&str; 4] = [
    "run.completed",
    "run.needs_input",
    "run.cancelled",
    "run.failed",
];

#[derive(Clone)]
pub struct Api {
    http: Client,
    base: String,
    user: Uuid,
}

impl Api {
    pub fn new(base: String, user: Uuid) -> Self {
        Self {
            http: Client::new(),
            base,
            user,
        }
    }

    fn request(&self, method: Method, path: &str) -> reqwest::RequestBuilder {
        self.http
            .request(method, format!("{}/v1{path}", self.base))
            .header("x-trex-user", self.user.to_string())
    }

    async fn call(&self, method: Method, path: &str, body: Option<Value>) -> anyhow::Result<Value> {
        let mut request = self.request(method.clone(), path);
        if let Some(body) = body {
            request = request.json(&body);
        }
        let response = request
            .send()
            .await
            .with_context(|| format!("{method} {path} failed"))?;
        let status = response.status();
        let text = response.text().await?;
        if !status.is_success() {
            bail!("{method} {path} returned {status}: {text}");
        }
        Ok(serde_json::from_str(&text).unwrap_or(Value::Null))
    }

    pub async fn create_session(
        &self,
        model: &str,
        effort: Option<&str>,
    ) -> anyhow::Result<String> {
        let session = self
            .call(
                Method::POST,
                "/sessions",
                Some(json!({"model": model, "reasoning_effort": effort})),
            )
            .await?;
        session["id"]
            .as_str()
            .map(str::to_owned)
            .context("session has no id")
    }

    pub async fn send(
        &self,
        session: &str,
        content: &str,
        interrupt: bool,
    ) -> anyhow::Result<Value> {
        self.send_with(session, content, interrupt, json!([])).await
    }

    pub async fn send_with(
        &self,
        session: &str,
        content: &str,
        interrupt: bool,
        attachments: Value,
    ) -> anyhow::Result<Value> {
        let body = json!({"content": content, "interrupt": interrupt, "attachments": attachments});
        self.call(
            Method::POST,
            &format!("/sessions/{session}/messages"),
            Some(body),
        )
        .await
    }

    pub async fn attachment(&self, id: &str) -> anyhow::Result<Vec<u8>> {
        let response = self
            .request(Method::GET, &format!("/attachments/{id}"))
            .send()
            .await?
            .error_for_status()?;
        Ok(response.bytes().await?.to_vec())
    }

    pub async fn answer(&self, session: &str, answers: Value) -> anyhow::Result<()> {
        self.call(
            Method::POST,
            &format!("/sessions/{session}/answers"),
            Some(json!({ "answers": answers })),
        )
        .await?;
        Ok(())
    }

    pub async fn session(&self, session: &str) -> anyhow::Result<Value> {
        self.call(Method::GET, &format!("/sessions/{session}"), None)
            .await
    }

    pub async fn items(&self, session: &str) -> anyhow::Result<Vec<Value>> {
        let items = self
            .call(Method::GET, &format!("/sessions/{session}/items"), None)
            .await?;
        Ok(items["data"].as_array().cloned().unwrap_or_default())
    }

    pub async fn access_requests(&self, session: &str) -> anyhow::Result<Vec<Value>> {
        let requests = self
            .call(
                Method::GET,
                &format!("/sessions/{session}/access_requests"),
                None,
            )
            .await?;
        Ok(requests["data"].as_array().cloned().unwrap_or_default())
    }

    pub async fn approve(&self, session: &str, request: &str) -> anyhow::Result<()> {
        self.call(
            Method::POST,
            &format!("/sessions/{session}/access_requests/{request}/approve"),
            None,
        )
        .await?;
        Ok(())
    }

    pub async fn delete_session(&self, session: &str) -> anyhow::Result<()> {
        self.call(Method::DELETE, &format!("/sessions/{session}"), None)
            .await?;
        Ok(())
    }

    pub async fn put_file(&self, path: &str, content: Vec<u8>) -> anyhow::Result<()> {
        let response = self
            .request(Method::PUT, &format!("/library/files/{path}"))
            .header("content-type", "application/octet-stream")
            .body(content)
            .send()
            .await?;
        if !response.status().is_success() {
            bail!("uploading {path} returned {}", response.status());
        }
        Ok(())
    }

    pub async fn get_file(&self, path: &str) -> anyhow::Result<Option<Vec<u8>>> {
        let response = self
            .request(Method::GET, &format!("/library/files/{path}"))
            .send()
            .await?;
        match response.status() {
            StatusCode::NOT_FOUND => Ok(None),
            status if status.is_success() => Ok(Some(response.bytes().await?.to_vec())),
            status => bail!("downloading {path} returned {status}"),
        }
    }

    pub async fn files(&self) -> anyhow::Result<Vec<String>> {
        let files = self.call(Method::GET, "/library", None).await?;
        Ok(files["data"]
            .as_array()
            .into_iter()
            .flatten()
            .filter_map(|file| file["path"].as_str().map(str::to_owned))
            .collect())
    }

    pub async fn delete_file(&self, path: &str) -> anyhow::Result<()> {
        self.call(Method::DELETE, &format!("/library/files/{path}"), None)
            .await?;
        Ok(())
    }

    // events are recorded into `sink` as they arrive, so a scenario's metrics survive its failure
    pub fn watch(&self, session: &str, sink: Arc<Mutex<Vec<Value>>>) -> Watch {
        Watch {
            api: self.clone(),
            session: session.to_owned(),
            stream: None,
            last_id: None,
            sink,
            seen: Vec::new(),
        }
    }
}

type EventStream =
    Pin<Box<dyn Stream<Item = Result<Event, EventStreamError<reqwest::Error>>> + Send>>;

// a session's event stream that reconnects with Last-Event-ID, including across trex restarts
pub struct Watch {
    api: Api,
    session: String,
    stream: Option<EventStream>,
    last_id: Option<String>,
    sink: Arc<Mutex<Vec<Value>>>,
    pub seen: Vec<Value>,
}

impl Watch {
    async fn connect(&mut self) -> anyhow::Result<()> {
        let path = match &self.last_id {
            Some(_) => format!("/sessions/{}/events", self.session),
            None => format!("/sessions/{}/events?from=start", self.session),
        };
        let mut request = self.api.request(Method::GET, &path);
        if let Some(id) = &self.last_id {
            request = request.header("last-event-id", id);
        }
        let response = request.send().await?.error_for_status()?;
        self.stream = Some(Box::pin(response.bytes_stream().eventsource()));
        Ok(())
    }

    pub async fn next(&mut self) -> anyhow::Result<Value> {
        loop {
            let Some(stream) = &mut self.stream else {
                if self.connect().await.is_err() {
                    sleep(Duration::from_millis(500)).await;
                }
                continue;
            };
            match stream.next().await {
                Some(Ok(event)) if event.data.is_empty() => {}
                Some(Ok(event)) => {
                    if !event.id.is_empty() {
                        self.last_id = Some(event.id);
                    }
                    let value: Value =
                        serde_json::from_str(&event.data).context("invalid event payload")?;
                    self.sink
                        .lock()
                        .expect("event sink lock poisoned")
                        .push(value.clone());
                    self.seen.push(value.clone());
                    return Ok(value);
                }
                Some(Err(_)) | None => {
                    self.stream = None;
                    sleep(Duration::from_millis(500)).await;
                }
            }
        }
    }

    pub async fn until(&mut self, matches: impl Fn(&Value) -> bool) -> anyhow::Result<Value> {
        loop {
            let event = self.next().await?;
            if matches(&event) {
                return Ok(event);
            }
        }
    }

    pub async fn until_type(&mut self, kind: &str) -> anyhow::Result<Value> {
        self.until(|event| event["type"] == kind).await
    }

    pub async fn until_end(&mut self) -> anyhow::Result<Value> {
        self.until(|event| RUN_ENDS.iter().any(|end| event["type"] == *end))
            .await
    }
}
