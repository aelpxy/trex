use std::{env, fs, net::SocketAddr, path::PathBuf};

use anyhow::{Context, bail};
use trex_harness::model::Models;

pub struct Config {
    pub addr: SocketAddr,
    pub log_format: LogFormat,
    pub openshell_endpoint: String,
    pub openshell_tls_dir: PathBuf,
    pub database_url: String,
    pub redis_url: String,
    pub models: Models,
}

#[derive(Clone, Copy)]
pub enum LogFormat {
    Text,
    Json,
}

impl Config {
    pub fn load() -> anyhow::Result<Self> {
        // variables already set in the environment take precedence over .env
        match dotenvy::dotenv() {
            Ok(_) => {}
            Err(error) if error.not_found() => {}
            Err(error) => return Err(error).context("invalid .env file"),
        }

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
        let models = Models::from_toml(&raw).with_context(|| format!("invalid {path}"))?;

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
