use std::time::Duration;

use anyhow::Context;
use uuid::Uuid;

use crate::Store;

// longer than a sandbox lives without being used, so a request is never announced twice
const ANNOUNCED_TTL: Duration = Duration::from_secs(24 * 60 * 60);

fn key(session: Uuid, request: &str) -> String {
    format!("trex:session:{session}:access:{request}")
}

impl Store {
    // true for the first caller on any instance, which then announces or answers the request
    pub async fn claim_access_request(&self, session: Uuid, request: &str) -> anyhow::Result<bool> {
        let mut redis = self.redis.clone();
        let claimed: Option<String> = redis::cmd("SET")
            .arg(key(session, request))
            .arg(1)
            .arg("NX")
            .arg("EX")
            .arg(ANNOUNCED_TTL.as_secs())
            .query_async(&mut redis)
            .await
            .context("failed to claim the access request")?;
        Ok(claimed.is_some())
    }
}
