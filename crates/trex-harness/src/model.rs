use std::collections::{HashMap, HashSet};

use anyhow::{Context, bail};
use async_openai::{
    Client,
    config::OpenAIConfig,
    error::OpenAIError,
    types::responses::{
        CreateResponse, EasyInputMessage, IncludeEnum, InputItem, InputParam, Reasoning,
        ReasoningEffort, ReasoningSummary, ResponseStream, ResponseStreamEvent,
        ServiceTierResponses, Tool, ToolChoiceOptions, ToolChoiceParam,
    },
};
use futures::StreamExt;
use serde::Deserialize;

// conservative for models whose catalog entry doesn't say; a low guess only compacts early
const DEFAULT_CONTEXT_WINDOW: u64 = 128_000;
const TITLE_PROMPT: &str = "Write a title of 2 to 6 words for a chat that starts with the user's message below, \
in the language of the message. Reply with only the title: no quotes, no punctuation at the end.";
const TITLE_INPUT_CHARS: usize = 2_000;
const DEFAULT_FAST_MULTIPLIER: f64 = 2.0;
const TITLE_MAX_CHARS: usize = 80;

pub struct Model {
    id: String,
    name: String,
    upstream: String,
    context_window: u64,
    // the levels the provider accepts; none means any level is passed through
    reasoning_efforts: Option<Vec<String>>,
    // whether the provider offers the priority service tier for this model
    fast: bool,
    price: Price,
    client: Client<OpenAIConfig>,
}

// credits per million tokens; a model without a price in the catalog is free
#[derive(Clone, Copy, Deserialize, Default)]
#[serde(deny_unknown_fields)]
pub struct Price {
    pub input: u64,
    pub cached_input: u64,
    pub output: u64,
    #[serde(default = "default_fast_multiplier")]
    pub fast_multiplier: f64,
}

fn default_fast_multiplier() -> f64 {
    DEFAULT_FAST_MULTIPLIER
}

pub struct Models {
    models: HashMap<String, Model>,
    order: Vec<String>,
}

pub struct Turn {
    pub instructions: Option<String>,
    pub input: Vec<InputItem>,
    pub tools: Vec<Tool>,
    pub reasoning_effort: Option<ReasoningEffort>,
    // the tools stay declared even when calls are disabled, so the cached prompt prefix still matches
    pub allow_tools: bool,
    pub cache_key: Option<String>,
    // the priority service tier: faster responses for more credits
    pub fast: bool,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    #[serde(default)]
    providers: HashMap<String, ProviderEntry>,
    #[serde(default)]
    models: Vec<ModelEntry>,
    // billing plans live in the same file but are read by the server
    #[serde(default)]
    #[allow(dead_code, reason = "parsed by the server's plan config")]
    plans: toml::Table,
    #[serde(default)]
    #[allow(dead_code, reason = "parsed by the transcribe tool's config")]
    transcription: toml::Table,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderEntry {
    base_url: String,
    api_key: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelEntry {
    id: String,
    name: Option<String>,
    provider: String,
    upstream: Option<String>,
    context_window: Option<u64>,
    reasoning_efforts: Option<Vec<String>>,
    #[serde(default)]
    fast: bool,
    price: Option<Price>,
}

// accepts the effort names the responses api uses, e.g. low, medium, high
pub fn parse_effort(effort: &str) -> anyhow::Result<ReasoningEffort> {
    serde_json::from_value(serde_json::Value::String(effort.to_owned()))
        .with_context(|| format!("unknown reasoning effort {effort}"))
}

impl Models {
    pub fn from_toml(raw: &str) -> anyhow::Result<Self> {
        let catalog: Catalog = toml::from_str(raw)?;

        let mut ids = HashSet::new();
        let mut models = HashMap::new();
        let mut order = Vec::new();
        for entry in catalog.models {
            let Some(provider) = catalog.providers.get(&entry.provider) else {
                bail!("model {}: unknown provider {}", entry.id, entry.provider);
            };
            if !ids.insert(entry.id.clone()) {
                bail!("duplicate model id {}", entry.id);
            }
            let context_window = entry.context_window.unwrap_or(DEFAULT_CONTEXT_WINDOW);
            if context_window == 0 {
                bail!("model {}: context_window must be positive", entry.id);
            }
            if let Some(efforts) = &entry.reasoning_efforts {
                if efforts.is_empty() {
                    bail!("model {}: reasoning_efforts must not be empty", entry.id);
                }
                for effort in efforts {
                    parse_effort(effort).with_context(|| format!("model {}", entry.id))?;
                }
            }
            let mut model = Model::new(
                entry.id.clone(),
                entry.name.unwrap_or_else(|| entry.id.clone()),
                entry.upstream.unwrap_or_else(|| entry.id.clone()),
                context_window,
                provider.base_url.trim_end_matches('/'),
                provider.api_key.as_deref(),
            );
            model.reasoning_efforts = entry.reasoning_efforts;
            model.fast = entry.fast;
            if let Some(price) = entry.price {
                if !(price.fast_multiplier >= 1.0 && price.fast_multiplier.is_finite()) {
                    bail!("model {}: fast_multiplier must be at least 1", entry.id);
                }
                model.price = price;
            }
            order.push(entry.id.clone());
            models.insert(entry.id, model);
        }

        if models.is_empty() {
            bail!("no models configured");
        }
        Ok(Self { models, order })
    }

    pub fn get(&self, id: &str) -> Option<&Model> {
        self.models.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.models.keys().map(String::as_str)
    }

    // in catalog order, which is the order the ui lists them
    pub fn all(&self) -> impl Iterator<Item = &Model> {
        self.order.iter().map(|id| &self.models[id])
    }
}

impl Model {
    fn new(
        id: String,
        name: String,
        upstream: String,
        context_window: u64,
        base_url: &str,
        api_key: Option<&str>,
    ) -> Self {
        let mut openai = OpenAIConfig::new().with_api_base(base_url);
        if let Some(key) = api_key {
            openai = openai.with_api_key(key);
        }

        Self {
            id,
            name,
            upstream,
            context_window,
            reasoning_efforts: None,
            fast: false,
            price: Price::default(),
            client: Client::with_config(openai),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub fn name(&self) -> &str {
        &self.name
    }

    pub fn context_window(&self) -> u64 {
        self.context_window
    }

    // the cheapest level the model accepts, for small side tasks like titles
    pub fn lowest_effort(&self) -> Option<ReasoningEffort> {
        let efforts = self.reasoning_efforts.as_ref()?;
        efforts.first().and_then(|effort| parse_effort(effort).ok())
    }

    // a short title for a conversation, from its first message
    pub async fn title(&self, message: &str) -> anyhow::Result<String> {
        let message: String = message.chars().take(TITLE_INPUT_CHARS).collect();
        let turn = Turn {
            instructions: Some(TITLE_PROMPT.into()),
            input: vec![EasyInputMessage::from(message).into()],
            tools: Vec::new(),
            reasoning_effort: self.lowest_effort(),
            allow_tools: true,
            cache_key: None,
            fast: false,
        };
        let mut stream = self.stream(turn).await?;
        let mut text = String::new();
        while let Some(event) = stream.next().await {
            match event? {
                ResponseStreamEvent::ResponseOutputTextDelta(delta) => text.push_str(&delta.delta),
                ResponseStreamEvent::ResponseCompleted(_) => break,
                ResponseStreamEvent::ResponseFailed(_)
                | ResponseStreamEvent::ResponseIncomplete(_) => {
                    bail!("the title request did not complete")
                }
                _ => {}
            }
        }
        let title = text
            .lines()
            .find(|line| !line.trim().is_empty())
            .unwrap_or_default()
            .trim()
            .trim_matches(|c| matches!(c, '"' | '\'' | '*' | '#' | '`'))
            .trim_end_matches('.')
            .trim();
        if title.is_empty() {
            bail!("the model returned an empty title");
        }
        Ok(title.chars().take(TITLE_MAX_CHARS).collect())
    }

    pub fn price(&self) -> Price {
        self.price
    }

    // what one model response costs, rounded up so even a tiny response is charged
    pub fn credits(&self, input: u64, cached_input: u64, output: u64, fast: bool) -> i64 {
        let uncached = input.saturating_sub(cached_input);
        let micro = u128::from(uncached) * u128::from(self.price.input)
            + u128::from(cached_input.min(input)) * u128::from(self.price.cached_input)
            + u128::from(output) * u128::from(self.price.output);
        let multiplier = if fast {
            self.price.fast_multiplier
        } else {
            1.0
        };
        let credits = (micro as f64 * multiplier / 1_000_000.0).ceil();
        credits.min(i64::MAX as f64) as i64
    }

    pub fn supports_fast(&self) -> bool {
        self.fast
    }

    pub fn reasoning_efforts(&self) -> Option<&[String]> {
        self.reasoning_efforts.as_deref()
    }

    // an unsupported level fails every request, so it is caught when a session picks it
    pub fn check_effort(&self, effort: &str) -> anyhow::Result<ReasoningEffort> {
        let parsed = parse_effort(effort)?;
        if let Some(efforts) = &self.reasoning_efforts
            && !efforts.iter().any(|allowed| allowed == effort)
        {
            bail!(
                "{} supports reasoning effort {}",
                self.id,
                efforts.join(", ")
            );
        }
        Ok(parsed)
    }

    pub async fn stream(&self, turn: Turn) -> Result<ResponseStream, OpenAIError> {
        // trex owns conversation state, so reasoning must come back encrypted to be replayed
        let request = CreateResponse {
            model: Some(self.upstream.clone()),
            instructions: turn.instructions,
            input: InputParam::Items(turn.input),
            tools: Some(turn.tools),
            tool_choice: (!turn.allow_tools)
                .then_some(ToolChoiceParam::Option(ToolChoiceOptions::None)),
            prompt_cache_key: turn.cache_key,
            // summaries are what the ui shows while the model thinks, so they're asked for even when
            // the effort is left to the provider
            reasoning: Some(Reasoning {
                effort: turn.reasoning_effort,
                summary: Some(ReasoningSummary::Auto),
                ..Default::default()
            }),
            include: Some(vec![IncludeEnum::ReasoningEncryptedContent]),
            service_tier: turn.fast.then_some(ServiceTierResponses::Priority),
            store: Some(false),
            ..Default::default()
        };

        self.client.responses().create_stream(request).await
    }
}

// live tests use the workspace trex.toml so no keys live in test code
#[cfg(test)]
pub(crate) const TEST_MODEL: &str = "gpt-6.1-sol";

#[cfg(test)]
fn test_catalog() -> String {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../trex.toml");
    std::fs::read_to_string(path).expect("live tests need trex.toml in the workspace root")
}

#[cfg(test)]
pub(crate) fn test_models() -> Models {
    Models::from_toml(&test_catalog()).unwrap()
}

#[cfg(test)]
fn test_entry() -> (ModelEntry, ProviderEntry) {
    let mut catalog: Catalog = toml::from_str(&test_catalog()).unwrap();
    let entry = catalog
        .models
        .into_iter()
        .find(|entry| entry.id == TEST_MODEL)
        .expect("trex.toml should configure the test model");
    let provider = catalog.providers.remove(&entry.provider).unwrap();
    (entry, provider)
}

#[cfg(test)]
pub(crate) fn test_base_url() -> String {
    test_entry().1.base_url.trim_end_matches('/').to_owned()
}

// the test model behind another base url (a fault-injecting proxy) with its own context window
#[cfg(test)]
pub(crate) fn test_model_via(base_url: &str, context_window: u64) -> Model {
    let (entry, provider) = test_entry();
    Model::new(
        entry.id.clone(),
        entry.id.clone(),
        entry.upstream.unwrap_or(entry.id),
        context_window,
        base_url,
        provider.api_key.as_deref(),
    )
}

#[cfg(test)]
mod tests {
    use async_openai::types::responses::{FunctionTool, OutputItem};
    use serde_json::json;

    use super::*;

    #[test]
    fn rejects_invalid_catalogs() {
        let provider = "[providers.p]\nbase_url = \"http://x/v1\"\n";
        let cases = [
            ("", "no models configured"),
            (
                "[[models]]\nid = \"a\"\nprovider = \"missing\"\n",
                "unknown provider",
            ),
            (
                &format!(
                    "{provider}[[models]]\nid = \"a\"\nprovider = \"p\"\n[[models]]\nid = \"a\"\nprovider = \"p\"\n"
                ),
                "duplicate model id",
            ),
            (&format!("{provider}region = \"x\"\n"), "unknown field"),
        ];
        for (raw, expected) in cases {
            let error = Models::from_toml(raw)
                .err()
                .expect("catalog should be rejected");
            assert!(
                format!("{error:#}").contains(expected),
                "{error:#} should mention {expected}"
            );
        }
    }

    #[test]
    fn loads_catalog() {
        let raw = r#"
            [providers.local]
            base_url = "http://127.0.0.1:8699/v1/"
            api_key = "secret"

            [[models]]
            id = "fast"
            name = "Fast"
            provider = "local"
            upstream = "gpt-6.1-sol"
            context_window = 400000
            reasoning_efforts = ["low", "high", "max"]
            fast = true
            price = { input = 100, cached_input = 10, output = 400 }

            [[models]]
            id = "plain"
            provider = "local"
        "#;
        let models = Models::from_toml(raw).unwrap();
        let mut ids: Vec<_> = models.ids().collect();
        ids.sort_unstable();
        assert_eq!(ids, ["fast", "plain"]);
        let fast = models.get("fast").unwrap();
        assert_eq!(fast.upstream, "gpt-6.1-sol");
        assert_eq!(fast.context_window(), 400_000);
        assert_eq!(
            models.all().map(Model::name).collect::<Vec<_>>(),
            ["Fast", "plain"]
        );
        assert_eq!(
            models.get("plain").unwrap().context_window(),
            DEFAULT_CONTEXT_WINDOW
        );
        assert_eq!(fast.check_effort("max").unwrap(), ReasoningEffort::Max);
        let unsupported = fast.check_effort("minimal").unwrap_err().to_string();
        assert_eq!(unsupported, "fast supports reasoning effort low, high, max");
        assert!(fast.check_effort("warp").is_err());
        let plain = models.get("plain").unwrap();
        assert_eq!(plain.reasoning_efforts(), None);
        assert!(fast.supports_fast() && !plain.supports_fast());
        // 9000 uncached at 100 + 1000 cached at 10 + 500 out at 400 = 1.11 credits, rounded up
        assert_eq!(fast.credits(10_000, 1_000, 500, false), 2);
        assert_eq!(fast.credits(10_000, 1_000, 500, true), 3);
        assert_eq!(fast.credits(1_000_000, 0, 0, false), 100);
        assert_eq!(fast.credits(0, 0, 0, true), 0);
        assert_eq!(
            plain.credits(1_000_000, 0, 1_000_000, false),
            0,
            "unpriced models are free"
        );
        assert_eq!(
            plain.check_effort("minimal").unwrap(),
            ReasoningEffort::Minimal
        );

        let invalid = raw.replace(r#"["low", "high", "max"]"#, r#"["low", "warp"]"#);
        assert!(Models::from_toml(&invalid).is_err());
    }

    // needs the responses api configured in trex.toml: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn writes_short_titles() {
        let models = test_models();
        let model = models.get(TEST_MODEL).unwrap();
        let title = model
            .title("my sse stream drops events when the client reconnects after a server restart, can you fix the resume logic?")
            .await
            .unwrap();
        let words = title.split_whitespace().count();
        assert!((2..=8).contains(&words), "{title}");
        assert!(!title.ends_with('.') && !title.starts_with('"'), "{title}");
    }

    // needs the responses api configured in trex.toml: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn streams_function_call() {
        let models = test_models();

        let turn = Turn {
            instructions: None,
            input: vec![EasyInputMessage::from("list files in /tmp using the bash tool").into()],
            tools: vec![Tool::Function(FunctionTool {
                name: "bash".into(),
                description: Some("run a shell command".into()),
                parameters: Some(json!({
                    "type": "object",
                    "properties": {"command": {"type": "string"}},
                    "required": ["command"],
                    "additionalProperties": false,
                })),
                strict: Some(true),
                ..Default::default()
            })],
            reasoning_effort: Some(ReasoningEffort::Medium),
            allow_tools: true,
            cache_key: None,
            fast: true,
        };

        let mut stream = models
            .get("gpt-6.1-sol")
            .unwrap()
            .stream(turn)
            .await
            .unwrap();
        let (mut calls, mut completed) = (Vec::new(), false);
        while let Some(event) = stream.next().await {
            match event.unwrap() {
                ResponseStreamEvent::ResponseOutputItemDone(done) => {
                    if let OutputItem::FunctionCall(call) = done.item {
                        calls.push(call);
                    }
                }
                ResponseStreamEvent::ResponseCompleted(_) => completed = true,
                _ => {}
            }
        }

        assert!(completed);
        assert_eq!(calls.len(), 1);
        assert_eq!(calls[0].name, "bash");
        let args: serde_json::Value = serde_json::from_str(&calls[0].arguments).unwrap();
        assert!(args["command"].as_str().unwrap().contains("/tmp"));
    }
}
