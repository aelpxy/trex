use std::collections::HashMap;

use anyhow::Context;
use async_openai::{
    Client,
    config::OpenAIConfig,
    types::responses::{
        CreateResponse, IncludeEnum, InputItem, InputParam, Reasoning, ReasoningEffort,
        ReasoningSummary, ResponseStream, Tool,
    },
};

pub struct ModelConfig {
    pub id: String,
    pub upstream: String,
    pub base_url: String,
    pub api_key: Option<String>,
}

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

impl Models {
    pub fn new(configs: Vec<ModelConfig>) -> Self {
        let models = configs
            .into_iter()
            .map(|config| (config.id.clone(), Model::new(config)))
            .collect();
        Self { models }
    }

    pub fn get(&self, id: &str) -> Option<&Model> {
        self.models.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.models.keys().map(String::as_str)
    }
}

impl Model {
    fn new(config: ModelConfig) -> Self {
        let mut openai = OpenAIConfig::new().with_api_base(config.base_url);
        if let Some(key) = config.api_key {
            openai = openai.with_api_key(key);
        }

        Self {
            id: config.id,
            upstream: config.upstream,
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

#[cfg(test)]
mod tests {
    use async_openai::types::responses::{
        EasyInputMessage, FunctionTool, OutputItem, ResponseStreamEvent,
    };
    use futures::StreamExt;
    use serde_json::json;

    use super::*;

    // needs the local responses api from trex.toml and LOCAL_API_KEY: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn streams_function_call() {
        let models = Models::new(vec![ModelConfig {
            id: "gpt-6.1-sol".into(),
            upstream: "gpt-6.1-sol".into(),
            base_url: "http://127.0.0.1:8699/v1".into(),
            api_key: std::env::var("LOCAL_API_KEY").ok(),
        }]);

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
