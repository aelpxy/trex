use std::time::Duration;

use anyhow::Context;
use redis::aio::ConnectionManager;
use sqlx::{PgPool, postgres::PgPoolOptions};

pub struct Store {
    pub pg: PgPool,
    pub redis: ConnectionManager,
}

impl Store {
    // connection urls carry credentials, so they never appear in errors or logs
    pub async fn connect(database_url: &str, redis_url: &str) -> anyhow::Result<Self> {
        let pg = PgPoolOptions::new()
            .max_connections(20)
            .acquire_timeout(Duration::from_secs(5))
            .connect(database_url)
            .await
            .context("failed to connect to postgres")?;

        sqlx::migrate!()
            .run(&pg)
            .await
            .context("failed to run database migrations")?;

        let redis = redis::Client::open(redis_url)
            .context("invalid redis url")?
            .get_connection_manager()
            .await
            .context("failed to connect to redis")?;

        Ok(Self { pg, redis })
    }

    pub async fn ping(&self) -> anyhow::Result<()> {
        sqlx::query("select 1")
            .execute(&self.pg)
            .await
            .context("postgres ping failed")?;
        redis::cmd("PING")
            .query_async::<()>(&mut self.redis.clone())
            .await
            .context("redis ping failed")?;
        Ok(())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // needs TREX_DATABASE_URL and TREX_REDIS_URL: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn connects_and_pings() {
        let database_url = std::env::var("TREX_DATABASE_URL").unwrap();
        let redis_url = std::env::var("TREX_REDIS_URL").unwrap();
        let store = Store::connect(&database_url, &redis_url).await.unwrap();
        store.ping().await.unwrap();
    }
}
