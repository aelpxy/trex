use std::collections::{HashMap, HashSet};

use anyhow::{Context, bail};
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::responses::{
        CreateResponse, IncludeEnum, InputItem, InputParam, Reasoning, ReasoningEffort,
        ReasoningSummary, ResponseStream, Tool,
    },
};
use serde::Deserialize;

pub struct Model {
    id: String,
    upstream: String,
    client: Client<OpenAIConfig>,
}

pub struct Models {
    models: HashMap<String, Model>,
}

pub struct Turn {
    pub instructions: Option<String>,
    pub input: Vec<InputItem>,
    pub tools: Vec<Tool>,
    pub reasoning_effort: Option<ReasoningEffort>,
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
    provider: String,
    upstream: Option<String>,
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
        for entry in catalog.models {
            let Some(provider) = catalog.providers.get(&entry.provider) else {
                bail!("model {}: unknown provider {}", entry.id, entry.provider);
            };
            if !ids.insert(entry.id.clone()) {
                bail!("duplicate model id {}", entry.id);
            }
            let model = Model::new(
                entry.id.clone(),
                entry.upstream.unwrap_or_else(|| entry.id.clone()),
                provider.base_url.trim_end_matches('/'),
                provider.api_key.as_deref(),
            );
            models.insert(entry.id, model);
        }

        if models.is_empty() {
            bail!("no models configured");
        }
        Ok(Self { models })
    }

    pub fn get(&self, id: &str) -> Option<&Model> {
        self.models.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.models.keys().map(String::as_str)
    }
}

impl Model {
    fn new(id: String, upstream: String, base_url: &str, api_key: Option<&str>) -> Self {
        let mut openai = OpenAIConfig::new().with_api_base(base_url);
        if let Some(key) = api_key {
            openai = openai.with_api_key(key);
        }

        Self {
            id,
            upstream,
            client: Client::with_config(openai),
        }
    }

    pub fn id(&self) -> &str {
        &self.id
    }

    pub async fn stream(&self, turn: Turn) -> anyhow::Result<ResponseStream> {
        // trex owns conversation state, so reasoning must come back encrypted to be replayed
        let request = CreateResponse {
            model: Some(self.upstream.clone()),
            instructions: turn.instructions,
            input: InputParam::Items(turn.input),
            tools: Some(turn.tools),
            reasoning: turn.reasoning_effort.map(|effort| Reasoning {
                effort: Some(effort),
                summary: Some(ReasoningSummary::Auto),
                ..Default::default()
            }),
            include: Some(vec![IncludeEnum::ReasoningEncryptedContent]),
            store: Some(false),
            ..Default::default()
        };

        self.client
            .responses()
            .create_stream(request)
            .await
            .with_context(|| format!("failed to start response for model {}", self.id))
    }
}

// live tests use the workspace trex.toml so no keys live in test code
#[cfg(test)]
pub(crate) const TEST_MODEL: &str = "gpt-6.1-sol";

#[cfg(test)]
pub(crate) fn test_models() -> Models {
    let path = concat!(env!("CARGO_MANIFEST_DIR"), "/../../trex.toml");
    let raw =
        std::fs::read_to_string(path).expect("live tests need trex.toml in the workspace root");
    Models::from_toml(&raw).unwrap()
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
            provider = "local"
            upstream = "gpt-6.1-sol"
        "#;
        let models = Models::from_toml(raw).unwrap();
        assert_eq!(models.ids().collect::<Vec<_>>(), ["fast"]);
        assert_eq!(models.get("fast").unwrap().upstream, "gpt-6.1-sol");
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
