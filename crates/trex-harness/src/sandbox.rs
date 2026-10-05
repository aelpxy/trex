use std::sync::atomic::{AtomicBool, Ordering};

use futures::future::BoxFuture;
use tokio::sync::{OnceCell, mpsc};
use trex_sandbox::Sandbox;

use crate::event::Event;

pub struct Provided {
    pub sandbox: Sandbox,
    // the session's earlier sandbox was lost, so this one starts empty
    pub replaced: bool,
}

// creates the session's sandbox, or starts it if it was stopped
pub trait SandboxProvider: Send + Sync {
    fn provide<'a>(
        &'a self,
        events: &'a mpsc::Sender<Event>,
    ) -> BoxFuture<'a, anyhow::Result<Provided>>;
}

// a conversation that never runs a tool never needs a sandbox, so it is made on first use
pub struct LazySandbox<'a> {
    cell: OnceCell<Sandbox>,
    provider: Option<&'a dyn SandboxProvider>,
    replaced: AtomicBool,
}

impl<'a> LazySandbox<'a> {
    pub fn new(provider: &'a dyn SandboxProvider) -> Self {
        Self {
            cell: OnceCell::new(),
            provider: Some(provider),
            replaced: AtomicBool::new(false),
        }
    }

    pub fn ready(sandbox: Sandbox) -> Self {
        Self {
            cell: OnceCell::new_with(Some(sandbox)),
            provider: None,
            replaced: AtomicBool::new(false),
        }
    }

    // concurrent tool calls share one provisioning; a failed one is retried by the next call
    pub async fn get(&self, events: &mpsc::Sender<Event>) -> anyhow::Result<&Sandbox> {
        self.cell
            .get_or_try_init(|| async {
                let provider = self
                    .provider
                    .expect("a lazy sandbox without a provider is created ready");
                let provided = provider.provide(events).await?;
                self.replaced.store(provided.replaced, Ordering::Relaxed);
                Ok(provided.sandbox)
            })
            .await
    }

    // true once, after provisioning had to replace a lost sandbox
    pub fn take_replaced(&self) -> bool {
        self.replaced.swap(false, Ordering::Relaxed)
    }

    pub fn get_if_ready(&self) -> Option<&Sandbox> {
        self.cell.get()
    }
}
