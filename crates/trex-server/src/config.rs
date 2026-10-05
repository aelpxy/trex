use std::{env, fs, net::SocketAddr, path::PathBuf, time::Duration};

use anyhow::{Context, bail};
use trex_harness::model::Models;
use trex_sandbox::Policy;

use crate::credits::Plans;
use trex_store::library::{Library, S3Config};

// restarting a stopped sandbox takes about a second, so idle ones are stopped early
const DEFAULT_SANDBOX_IDLE_TIMEOUT: Duration = Duration::from_secs(300);

pub struct Config {
    pub addr: SocketAddr,
    // serves running apps from sandboxes, each on its own origin
    pub preview_addr: SocketAddr,
    pub preview_url: PreviewUrl,
    pub log_format: LogFormat,
    pub openshell_endpoint: String,
    pub openshell_tls_dir: PathBuf,
    pub database_url: String,
    pub redis_url: String,
    pub models: Models,
    pub plans: Plans,
    pub sandbox_image: String,
    pub sandbox_policy: Policy,
    pub sandbox_idle_timeout: Duration,
    pub library: Library,
}

// where a preview is reached, with `{id}` as the first label of the host so every preview is its own
// origin and can't read the app's or another preview's data
#[derive(Clone)]
pub struct PreviewUrl(String);

impl PreviewUrl {
    fn parse(template: &str) -> anyhow::Result<Self> {
        let host = template
            .split_once("://")
            .map(|(_, rest)| rest)
            .context("TREX_PREVIEW_URL needs a scheme, e.g. http://{id}.preview.localhost:8081")?;
        if !host.starts_with("{id}.") {
            bail!(
                "TREX_PREVIEW_URL must start its host with {{id}}., e.g. http://{{id}}.preview.localhost:8081"
            );
        }
        Ok(Self(template.trim_end_matches('/').to_owned()))
    }

    pub fn of(&self, id: &str) -> String {
        format!("{}/", self.0.replace("{id}", id))
    }
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

        let preview_addr = env::var("TREX_PREVIEW_ADDR")
            .unwrap_or_else(|_| "127.0.0.1:8081".into())
            .parse()
            .context("invalid TREX_PREVIEW_ADDR")?;
        let preview_url = PreviewUrl::parse(
            &env::var("TREX_PREVIEW_URL")
                .unwrap_or_else(|_| "http://{id}.preview.localhost:8081".into()),
        )?;

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

        let sandbox_image = env::var("TREX_SANDBOX_IMAGE")
            .unwrap_or_else(|_| "localhost/trex-sandbox:latest".into());

        let policy_path =
            env::var("TREX_SANDBOX_POLICY").unwrap_or_else(|_| "sandbox-policy.yaml".into());
        let policy_yaml = fs::read_to_string(&policy_path)
            .with_context(|| format!("failed to read {policy_path}"))?;
        let sandbox_policy =
            Policy::from_yaml(&policy_yaml).with_context(|| format!("invalid {policy_path}"))?;

        let sandbox_idle_timeout = match env::var("TREX_SANDBOX_IDLE_SECS") {
            Ok(secs) => {
                Duration::from_secs(secs.parse().context("invalid TREX_SANDBOX_IDLE_SECS")?)
            }
            Err(_) => DEFAULT_SANDBOX_IDLE_TIMEOUT,
        };

        let library = load_library()?;

        let path = env::var("TREX_CONFIG").unwrap_or_else(|_| "trex.toml".into());
        let raw = fs::read_to_string(&path).with_context(|| format!("failed to read {path}"))?;
        let models = Models::from_toml(&raw).with_context(|| format!("invalid {path}"))?;
        let plans = Plans::from_toml(&raw).with_context(|| format!("invalid plans in {path}"))?;
        Ok(Self {
            addr,
            preview_addr,
            preview_url,
            log_format,
            openshell_endpoint,
            openshell_tls_dir,
            database_url,
            redis_url,
            models,
            plans,
            sandbox_image,
            sandbox_policy,
            sandbox_idle_timeout,
            library,
        })
    }
}

// s3 is used when a bucket is configured; otherwise files live in a local directory for development
fn load_library() -> anyhow::Result<Library> {
    let Ok(bucket) = env::var("TREX_S3_BUCKET") else {
        let dir = env::var("TREX_LIBRARY_DIR").unwrap_or_else(|_| "data/library".into());
        return Library::local(&PathBuf::from(dir));
    };
    let required = |name: &str| env::var(name).with_context(|| format!("{name} is not set"));
    let force_path_style = match env::var("TREX_S3_FORCE_PATH_STYLE").as_deref() {
        Err(_) | Ok("false") => false,
        Ok("true") => true,
        Ok(other) => bail!("invalid TREX_S3_FORCE_PATH_STYLE: {other} (expected true or false)"),
    };
    Library::s3(S3Config {
        endpoint: env::var("TREX_S3_ENDPOINT").ok(),
        region: env::var("TREX_S3_REGION").unwrap_or_else(|_| "us-east-1".into()),
        bucket,
        access_key_id: required("TREX_S3_ACCESS_KEY_ID")?,
        secret_access_key: required("TREX_S3_SECRET_ACCESS_KEY")?,
        force_path_style,
    })
}
