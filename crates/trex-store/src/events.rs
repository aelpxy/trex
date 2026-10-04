use std::time::Duration;

use anyhow::Context;
use redis::{
    AsyncCommands, AsyncConnectionConfig,
    aio::MultiplexedConnection,
    streams::{StreamMaxlen, StreamReadOptions, StreamReadReply},
};
use serde_json::Value;
use uuid::Uuid;

use crate::Store;

const MAX_EVENTS_PER_SESSION: usize = 10_000;
const EVENT_RETENTION: Duration = Duration::from_secs(7 * 24 * 60 * 60);
const READ_BATCH: usize = 200;

pub struct StoredEvent {
    pub id: String,
    pub event: Value,
}

fn key(session: Uuid) -> String {
    format!("trex:session:{session}:events")
}

// session events go through a redis stream so sse clients on any instance can resume by event id
impl Store {
    pub async fn publish_event(&self, session: Uuid, event: &Value) -> anyhow::Result<String> {
        let key = key(session);
        let mut redis = self.redis.clone();
        let id: String = redis
            .xadd_maxlen(
                &key,
                StreamMaxlen::Approx(MAX_EVENTS_PER_SESSION),
                "*",
                &[("data", event.to_string())],
            )
            .await
            .context("failed to publish event")?;
        let _: () = redis
            .expire(&key, EVENT_RETENTION.as_secs() as i64)
            .await
            .context("failed to set event retention")?;
        Ok(id)
    }

    // blocking reads would stall the shared connection, so each reader gets its own, without the
    // default 500ms response timeout that a blocking read always exceeds
    pub async fn event_connection(&self) -> anyhow::Result<MultiplexedConnection> {
        let config = AsyncConnectionConfig::new().set_response_timeout(None);
        self.redis_client
            .get_multiplexed_async_connection_with_config(&config)
            .await
            .context("failed to open redis connection")
    }

    pub async fn read_events(
        &self,
        connection: &mut MultiplexedConnection,
        session: Uuid,
        after: &str,
        block: Duration,
    ) -> anyhow::Result<Vec<StoredEvent>> {
        let options = StreamReadOptions::default()
            .count(READ_BATCH)
            .block(block.as_millis() as usize);
        let reply: Option<StreamReadReply> = connection
            .xread_options(&[key(session)], &[after], &options)
            .await
            .context("failed to read events")?;

        let mut events = Vec::new();
        for stream in reply.map(|reply| reply.keys).unwrap_or_default() {
            for entry in stream.ids {
                let data: String = entry.get("data").context("event without data")?;
                let event = serde_json::from_str(&data).context("invalid event data")?;
                events.push(StoredEvent {
                    id: entry.id,
                    event,
                });
            }
        }
        Ok(events)
    }

    pub async fn last_event_id(&self, session: Uuid) -> anyhow::Result<Option<String>> {
        let reply: redis::streams::StreamRangeReply = self
            .redis
            .clone()
            .xrevrange_count(key(session), "+", "-", 1)
            .await
            .context("failed to read last event")?;
        Ok(reply.ids.into_iter().next().map(|entry| entry.id))
    }

    pub async fn delete_events(&self, session: Uuid) -> anyhow::Result<()> {
        let _: () = self
            .redis
            .clone()
            .del(key(session))
            .await
            .context("failed to delete events")?;
        Ok(())
    }
}
