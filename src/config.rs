use std::{env, net::SocketAddr, path::PathBuf};

use anyhow::{Context, bail};

pub struct Config {
    pub addr: SocketAddr,
    pub log_format: LogFormat,
    pub openshell_endpoint: String,
    pub openshell_tls_dir: PathBuf,
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

        let openshell_endpoint = env::var("TREX_OPENSHELL_ENDPOINT")
            .unwrap_or_else(|_| "https://127.0.0.1:17670".into());

        let openshell_tls_dir = PathBuf::from(
            env::var("TREX_OPENSHELL_TLS_DIR").unwrap_or_else(|_| "certs/openshell".into()),
        );

        Ok(Self {
            addr,
            log_format,
            openshell_endpoint,
            openshell_tls_dir,
        })
    }
}
