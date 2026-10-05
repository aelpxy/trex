use std::time::Duration;

use anyhow::Context;
use redis::AsyncCommands;
use uuid::Uuid;

use crate::Store;

// a preview link works for a day; opening the preview again makes a new one
pub const PREVIEW_TTL: Duration = Duration::from_secs(24 * 60 * 60);

// a running app in a session's sandbox, reachable at a capability url while the key lives
pub struct Preview {
    pub workspace: Uuid,
    pub session: Uuid,
    pub port: u16,
}

fn key(id: &str) -> String {
    format!("trex:preview:{id}")
}

impl Store {
    pub async fn create_preview(&self, id: &str, preview: &Preview) -> anyhow::Result<()> {
        let value = format!("{}:{}:{}", preview.workspace, preview.session, preview.port);
        let mut redis = self.redis.clone();
        let _: () = redis
            .set_ex(key(id), value, PREVIEW_TTL.as_secs())
            .await
            .context("failed to save the preview")?;
        Ok(())
    }

    pub async fn preview(&self, id: &str) -> anyhow::Result<Option<Preview>> {
        let mut redis = self.redis.clone();
        let value: Option<String> = redis
            .get(key(id))
            .await
            .context("failed to look up the preview")?;
        let Some(value) = value else {
            return Ok(None);
        };
        let mut parts = value.splitn(3, ':');
        let (Some(workspace), Some(session), Some(port)) =
            (parts.next(), parts.next(), parts.next())
        else {
            anyhow::bail!("malformed preview record");
        };
        Ok(Some(Preview {
            workspace: workspace.parse().context("malformed preview workspace")?,
            session: session.parse().context("malformed preview session")?,
            port: port.parse().context("malformed preview port")?,
        }))
    }
}
