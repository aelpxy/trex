use std::{
    collections::{HashMap, HashSet},
    env, fs,
    net::SocketAddr,
    path::PathBuf,
};

use anyhow::{Context, bail};
use serde::Deserialize;
use trex_harness::model::ModelConfig;

pub struct Config {
    pub addr: SocketAddr,
    pub log_format: LogFormat,
    pub openshell_endpoint: String,
    pub openshell_tls_dir: PathBuf,
    pub database_url: String,
    pub redis_url: String,
    pub models: Vec<ModelConfig>,
}

#[derive(Clone, Copy)]
pub enum LogFormat {
    Text,
    Json,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct File {
    #[serde(default)]
    providers: HashMap<String, ProviderEntry>,
    #[serde(default)]
    models: Vec<ModelEntry>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ProviderEntry {
    base_url: String,
    api_key_env: Option<String>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
struct ModelEntry {
    id: String,
    provider: String,
    upstream: Option<String>,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        let addr = env::var("TREX_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8080".into())
            .parse()
            .context("invalid TREX_ADDR")?;

        let log_format = match env::var("TREX_LOG_FORMAT").as_deref() {
            Err(_) | Ok("text") => LogFormat::Text,
            Ok("json") => LogFormat::Json,
            Ok(other) => bail!("invalid TREX_LOG_FORMAT: {other} (expected text or json)"),
        };

        let openshell_endpoint = env::var("TREX_OPENSHELL_ENDPOINT")
            .unwrap_or_else(|_| "https://127.0.0.1:17670".into());

        let openshell_tls_dir = PathBuf::from(
            env::var("TREX_OPENSHELL_TLS_DIR").unwrap_or_else(|_| "certs/openshell".into()),
        );

        let database_url = env::var("TREX_DATABASE_URL").context("TREX_DATABASE_URL is not set")?;
        let redis_url = env::var("TREX_REDIS_URL").context("TREX_REDIS_URL is not set")?;

        let path = env::var("TREX_CONFIG").unwrap_or_else(|_| "trex.toml".into());
        let raw = fs::read_to_string(&path).with_context(|| format!("failed to read {path}"))?;
        let file: File = toml::from_str(&raw).with_context(|| format!("invalid {path}"))?;
        let models = resolve(file)?;

        Ok(Self {
            addr,
            log_format,
            openshell_endpoint,
            openshell_tls_dir,
            database_url,
            redis_url,
            models,
        })
    }
}

fn resolve(file: File) -> anyhow::Result<Vec<ModelConfig>> {
    let mut ids = HashSet::new();
    let mut models = Vec::new();
    for entry in file.models {
        let Some(provider) = file.providers.get(&entry.provider) else {
            bail!("model {}: unknown provider {}", entry.id, entry.provider);
        };
        if !ids.insert(entry.id.clone()) {
            bail!("duplicate model id {}", entry.id);
        }

        let api_key = match &provider.api_key_env {
            Some(var) => Some(env::var(var).with_context(|| {
                format!(
                    "provider {}: environment variable {var} is not set",
                    entry.provider
                )
            })?),
            None => None,
        };

        models.push(ModelConfig {
            upstream: entry.upstream.unwrap_or_else(|| entry.id.clone()),
            id: entry.id,
            base_url: provider.base_url.trim_end_matches('/').to_owned(),
            api_key,
        });
    }

    if models.is_empty() {
        bail!("no models configured");
    }

    Ok(models)
}
