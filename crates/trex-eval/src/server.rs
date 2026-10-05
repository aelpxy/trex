use std::{
    fs::{self, File},
    net::TcpListener,
    path::{Path, PathBuf},
    process::Stdio,
    time::Duration,
};

use anyhow::{Context, bail};
use tokio::{
    process::{Child, Command},
    sync::Mutex,
    time::{Instant, sleep},
};

pub const MODEL: &str = "gpt-6.1-sol";
// the same model with a tiny window, so a short task has to compact its context
pub const SMALL_CONTEXT_MODEL: &str = "eval-small-context";
const SMALL_CONTEXT_WINDOW: i64 = 8_000;
// short, so the idle-stop scenario doesn't take long
const SANDBOX_IDLE_SECS: &str = "15";
const EVAL_MONTHLY_CREDITS: i64 = 100_000_000;

#[derive(serde::Serialize)]
struct EvalPrice {
    input: u64,
    cached_input: u64,
    output: u64,
}

const EVAL_PRICE: EvalPrice = EvalPrice {
    input: 1_000,
    cached_input: 100,
    output: 4_000,
};
const START_TIMEOUT: Duration = Duration::from_secs(60);

// a trex instance owned by the eval, so scenarios can crash and restart it
pub struct Server {
    root: PathBuf,
    dir: PathBuf,
    port: u16,
    preview_port: u16,
    child: Mutex<Option<Child>>,
}

impl Server {
    pub async fn start(root: &Path) -> anyhow::Result<Self> {
        let status = Command::new("cargo")
            .args(["build", "--quiet", "--bin", "trex"])
            .current_dir(root)
            .status()
            .await
            .context("failed to run cargo build")?;
        if !status.success() {
            bail!("building trex failed");
        }

        let dir = root.join("target/eval");
        fs::create_dir_all(&dir).context("failed to create target/eval")?;
        write_config(root, &dir)?;
        let port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();
        let preview_port = TcpListener::bind("127.0.0.1:0")?.local_addr()?.port();

        let server = Self {
            root: root.to_owned(),
            dir,
            port,
            preview_port,
            child: Mutex::new(None),
        };
        server.restart().await?;
        Ok(server)
    }

    pub fn url(&self) -> String {
        format!("http://127.0.0.1:{}", self.port)
    }

    // makes an existing account an admin, the way an operator would
    pub async fn grant_admin(&self, email: &str) -> anyhow::Result<()> {
        let status = Command::new(self.root.join("target/debug/trex"))
            .current_dir(&self.root)
            .args(["admin", "grant", email])
            .env("TREX_CONFIG", self.dir.join("trex.toml"))
            .stdout(Stdio::null())
            .status()
            .await
            .context("failed to run trex admin grant")?;
        if !status.success() {
            bail!("trex admin grant {email} failed");
        }
        Ok(())
    }

    // previews are served here, picked by the host header's first label
    pub fn preview_addr(&self) -> String {
        format!("127.0.0.1:{}", self.preview_port)
    }

    pub fn log_path(&self) -> PathBuf {
        self.dir.join("trex.log")
    }

    // sigkill, like a crash: nothing gets to shut down cleanly
    pub async fn kill(&self) -> anyhow::Result<()> {
        if let Some(mut child) = self.child.lock().await.take() {
            child.kill().await.context("failed to kill trex")?;
        }
        Ok(())
    }

    pub async fn restart(&self) -> anyhow::Result<()> {
        let mut child = self.child.lock().await;
        if child.is_some() {
            bail!("trex is already running");
        }
        let log = File::options()
            .create(true)
            .append(true)
            .open(self.log_path())
            .context("failed to open the trex log")?;
        let spawned = Command::new(self.root.join("target/debug/trex"))
            .current_dir(&self.root)
            .env("TREX_ADDR", format!("127.0.0.1:{}", self.port))
            .env("TREX_PREVIEW_ADDR", self.preview_addr())
            .env(
                "TREX_PREVIEW_URL",
                format!("http://{{id}}.preview.localhost:{}", self.preview_port),
            )
            .env("TREX_CONFIG", self.dir.join("trex.toml"))
            .env("TREX_SANDBOX_IDLE_SECS", SANDBOX_IDLE_SECS)
            .env("TREX_LOG_FORMAT", "text")
            .env("RUST_LOG", "info")
            .stdout(Stdio::from(log.try_clone()?))
            .stderr(Stdio::from(log))
            .kill_on_drop(true)
            .spawn()
            .context("failed to start trex")?;
        *child = Some(spawned);
        drop(child);
        self.wait_healthy().await
    }

    async fn wait_healthy(&self) -> anyhow::Result<()> {
        let http = reqwest::Client::new();
        let deadline = Instant::now() + START_TIMEOUT;
        loop {
            let health = http.get(format!("{}/health", self.url())).send().await;
            if health.is_ok_and(|response| response.status().is_success()) {
                return Ok(());
            }
            if Instant::now() > deadline {
                bail!(
                    "trex did not become healthy; see {}",
                    self.log_path().display()
                );
            }
            sleep(Duration::from_millis(250)).await;
        }
    }
}

// the operator's catalog plus a copy of the eval model with a tiny context window
fn write_config(root: &Path, dir: &Path) -> anyhow::Result<()> {
    let raw = fs::read_to_string(root.join("trex.toml")).context("failed to read trex.toml")?;
    let mut config: toml::Table = raw.parse().context("invalid trex.toml")?;
    let models = config
        .get_mut("models")
        .and_then(|models| models.as_array_mut())
        .context("trex.toml has no models")?;
    let mut small = models
        .iter()
        .find(|model| model.get("id").and_then(|id| id.as_str()) == Some(MODEL))
        .with_context(|| format!("trex.toml has no {MODEL} model"))?
        .clone();
    let small_table = small.as_table_mut().context("invalid model entry")?;
    let upstream = small_table
        .get("upstream")
        .cloned()
        .unwrap_or_else(|| MODEL.into());
    small_table.insert("id".into(), SMALL_CONTEXT_MODEL.into());
    small_table.insert("name".into(), "Eval small context".into());
    small_table.insert("upstream".into(), upstream);
    small_table.insert("context_window".into(), SMALL_CONTEXT_WINDOW.into());
    // charging needs prices, and a generous plan keeps long eval runs going
    for model in models.iter_mut().filter_map(|model| model.as_table_mut()) {
        model.entry("price").or_insert_with(|| {
            toml::Value::try_from(EVAL_PRICE).expect("the eval price is valid toml")
        });
    }
    models.push(small);
    let mut free = toml::Table::new();
    free.insert("name".into(), "Free".into());
    free.insert("monthly_credits".into(), EVAL_MONTHLY_CREDITS.into());
    let mut plans = toml::Table::new();
    plans.insert("free".into(), free.into());
    config.insert("plans".into(), plans.into());
    // the copy holds provider api keys, which is fine under the gitignored target directory
    fs::write(dir.join("trex.toml"), toml::to_string(&config)?)
        .context("failed to write the eval config")
}
