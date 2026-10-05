use futures::future::BoxFuture;
use tokio::sync::{OnceCell, mpsc};
use trex_sandbox::Sandbox;

use crate::event::Event;

// creates the session's sandbox, or starts it if it was stopped
pub trait SandboxProvider: Send + Sync {
    fn provide<'a>(
        &'a self,
        events: &'a mpsc::Sender<Event>,
    ) -> BoxFuture<'a, anyhow::Result<Sandbox>>;
}

// a conversation that never runs a tool never needs a sandbox, so it is made on first use
pub struct LazySandbox<'a> {
    cell: OnceCell<Sandbox>,
    provider: Option<&'a dyn SandboxProvider>,
}

impl<'a> LazySandbox<'a> {
    pub fn new(provider: &'a dyn SandboxProvider) -> Self {
        Self {
            cell: OnceCell::new(),
            provider: Some(provider),
        }
    }

    pub fn ready(sandbox: Sandbox) -> Self {
        Self {
            cell: OnceCell::new_with(Some(sandbox)),
            provider: None,
        }
    }

    // concurrent tool calls share one provisioning; a failed one is retried by the next call
    pub async fn get(&self, events: &mpsc::Sender<Event>) -> anyhow::Result<&Sandbox> {
        self.cell
            .get_or_try_init(|| async {
                let provider = self
                    .provider
                    .expect("a lazy sandbox without a provider is created ready");
                provider.provide(events).await
            })
            .await
    }

    pub fn get_if_ready(&self) -> Option<&Sandbox> {
        self.cell.get()
    }
}
