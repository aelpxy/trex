use std::{env, net::SocketAddr};

use anyhow::{Context, bail};

pub struct Config {
    pub addr: SocketAddr,
    pub log_format: LogFormat,
}

#[derive(Clone, Copy)]
pub enum LogFormat {
    Text,
    Json,
}

impl Config {
    pub fn from_env() -> anyhow::Result<Self> {
        let addr = env::var("TREX_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8080".into())
            .parse()
            .context("invalid TREX_ADDR")?;

        let log_format = match env::var("TREX_LOG_FORMAT").as_deref() {
            Err(_) | Ok("text") => LogFormat::Text,
            Ok("json") => LogFormat::Json,
            Ok(other) => bail!("invalid TREX_LOG_FORMAT: {other} (expected text or json)"),
        };

        Ok(Self { addr, log_format })
    }
}
