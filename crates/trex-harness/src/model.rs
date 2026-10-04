use std::collections::{HashMap, HashSet};

use anyhow::{Context, bail};
use async_openai::{
    Client,
    config::OpenAIConfig,
    error::OpenAIError,
    types::responses::{
        CreateResponse, IncludeEnum, InputItem, InputParam, Reasoning, ReasoningEffort,
        ReasoningSummary, ResponseStream, Tool, ToolChoiceOptions, ToolChoiceParam,
    },
};
use serde::Deserialize;

// conservative for models whose catalog entry doesn't say; a low guess only compacts early
const DEFAULT_CONTEXT_WINDOW: u64 = 128_000;

pub struct Model {
    id: String,
    name: String,
    upstream: String,
    context_window: u64,
    client: Client<OpenAIConfig>,
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
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct Catalog {
    #[serde(default)]
    providers: HashMap<String, ProviderEntry>,
    #[serde(default)]
    models: Vec<ModelEntry>,
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
            let model = Model::new(
                entry.id.clone(),
                entry.name.unwrap_or_else(|| entry.id.clone()),
                entry.upstream.unwrap_or_else(|| entry.id.clone()),
                context_window,
                provider.base_url.trim_end_matches('/'),
                provider.api_key.as_deref(),
            );
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
            reasoning: turn.reasoning_effort.map(|effort| Reasoning {
                effort: Some(effort),
                summary: Some(ReasoningSummary::Auto),
                ..Default::default()
            }),
            include: Some(vec![IncludeEnum::ReasoningEncryptedContent]),
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
    use async_openai::types::responses::{
        EasyInputMessage, FunctionTool, OutputItem, ResponseStreamEvent,
    };
    use futures::StreamExt;
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
