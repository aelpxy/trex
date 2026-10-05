use anyhow::Context;
use serde_json::Value;
use sqlx::Row;
use uuid::Uuid;

use crate::Store;

pub struct UsageRecord<'a> {
    pub workspace_id: Uuid,
    pub session_id: Uuid,
    pub model: &'a str,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    pub duration_ms: u64,
    pub first_token_ms: Option<u64>,
}

// one model response's usage, for showing what each turn of a chat cost
pub struct UsageEntry {
    pub created_at_ms: i64,
    pub model: String,
    pub input_tokens: i64,
    pub cached_input_tokens: i64,
    pub cache_write_tokens: i64,
    pub output_tokens: i64,
    pub reasoning_tokens: i64,
    pub credits: i64,
    pub duration_ms: i64,
    pub first_token_ms: Option<i64>,
}

impl Store {
    pub async fn session_items(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<Vec<Value>> {
        let rows = sqlx::query(
            "SELECT i.item FROM session_items i JOIN sessions s ON s.id = i.session_id \
             WHERE i.session_id = $1 AND s.workspace_id = $2 ORDER BY i.seq",
        )
        .bind(id)
        .bind(workspace)
        .fetch_all(&self.pg)
        .await
        .context("failed to load session items")?;
        rows.iter()
            .map(|row| row.try_get("item").context("invalid session item"))
            .collect()
    }

    // with when each item was saved, in unix milliseconds
    pub async fn session_items_timed(
        &self,
        workspace: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Vec<(Value, i64, i64)>> {
        let rows = sqlx::query(
            "SELECT i.item, i.seq, (EXTRACT(EPOCH FROM i.created_at) * 1000)::BIGINT AS created_at_ms \
             FROM session_items i JOIN sessions s ON s.id = i.session_id \
             WHERE i.session_id = $1 AND s.workspace_id = $2 ORDER BY i.seq",
        )
        .bind(id)
        .bind(workspace)
        .fetch_all(&self.pg)
        .await
        .context("failed to load session items")?;
        rows.iter()
            .map(|row| {
                Ok((
                    row.try_get("item")?,
                    row.try_get("seq")?,
                    row.try_get("created_at_ms")?,
                ))
            })
            .collect()
    }

    pub async fn count_session_items(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<i64> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM session_items i JOIN sessions s ON s.id = i.session_id \
             WHERE i.session_id = $1 AND s.workspace_id = $2",
        )
        .bind(id)
        .bind(workspace)
        .fetch_one(&self.pg)
        .await
        .context("failed to count session items")
    }

    pub async fn count_session_usage(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<i64> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM usage_records WHERE session_id = $1 AND workspace_id = $2",
        )
        .bind(id)
        .bind(workspace)
        .fetch_one(&self.pg)
        .await
        .context("failed to count session usage")
    }

    pub async fn session_usage(
        &self,
        workspace: Uuid,
        id: Uuid,
    ) -> anyhow::Result<Vec<UsageEntry>> {
        let rows = sqlx::query(
            "SELECT (EXTRACT(EPOCH FROM created_at) * 1000)::BIGINT AS created_at_ms, model, input_tokens, \
             cached_input_tokens, cache_write_tokens, output_tokens, reasoning_tokens, credits, duration_ms, first_token_ms \
             FROM usage_records WHERE session_id = $1 AND workspace_id = $2 ORDER BY created_at",
        )
        .bind(id)
        .bind(workspace)
        .fetch_all(&self.pg)
        .await
        .context("failed to load session usage")?;
        rows.iter()
            .map(|row| {
                Ok(UsageEntry {
                    created_at_ms: row.try_get("created_at_ms")?,
                    model: row.try_get("model")?,
                    input_tokens: row.try_get("input_tokens")?,
                    cached_input_tokens: row.try_get("cached_input_tokens")?,
                    cache_write_tokens: row.try_get("cache_write_tokens")?,
                    output_tokens: row.try_get("output_tokens")?,
                    reasoning_tokens: row.try_get("reasoning_tokens")?,
                    credits: row.try_get("credits")?,
                    duration_ms: row.try_get("duration_ms")?,
                    first_token_ms: row.try_get("first_token_ms")?,
                })
            })
            .collect()
    }

    pub async fn append_session_items(
        &self,
        workspace: Uuid,
        id: Uuid,
        items: &[Value],
    ) -> anyhow::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO session_items (session_id, seq, item) \
             SELECT s.id, (SELECT COALESCE(MAX(seq), 0) FROM session_items WHERE session_id = s.id) + t.ord, t.item \
             FROM sessions s, UNNEST($3::JSONB[]) WITH ORDINALITY AS t (item, ord) \
             WHERE s.id = $1 AND s.workspace_id = $2",
        )
        .bind(id)
        .bind(workspace)
        .bind(items)
        .execute(&self.pg)
        .await
        .context("failed to save session items")?;
        Ok(())
    }

    // items are keyed by their position in history, so saving the same items again is a no-op
    pub async fn put_session_items(
        &self,
        workspace: Uuid,
        id: Uuid,
        first: usize,
        items: &[Value],
    ) -> anyhow::Result<()> {
        if items.is_empty() {
            return Ok(());
        }
        sqlx::query(
            "INSERT INTO session_items (session_id, seq, item) \
             SELECT s.id, $3 + t.ord, t.item \
             FROM sessions s, UNNEST($4::JSONB[]) WITH ORDINALITY AS t (item, ord) \
             WHERE s.id = $1 AND s.workspace_id = $2 \
             ON CONFLICT (session_id, seq) DO NOTHING",
        )
        .bind(id)
        .bind(workspace)
        .bind(first as i64)
        .bind(items)
        .execute(&self.pg)
        .await
        .context("failed to save session items")?;
        Ok(())
    }
}
