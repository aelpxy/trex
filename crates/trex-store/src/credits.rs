use anyhow::Context;
use sqlx::{FromRow, Row, postgres::PgRow};
use uuid::Uuid;

use crate::{Store, sessions::UsageRecord};

pub struct LedgerEntry {
    pub id: Uuid,
    pub amount: i64,
    pub balance: i64,
    pub kind: String,
    pub description: String,
    pub created_at: i64,
}

impl FromRow<'_, PgRow> for LedgerEntry {
    fn from_row(row: &PgRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            amount: row.try_get("amount")?,
            balance: row.try_get("balance")?,
            kind: row.try_get("kind")?,
            description: row.try_get("description")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl Store {
    pub async fn credit_balance(&self, workspace: Uuid) -> anyhow::Result<i64> {
        sqlx::query_scalar("SELECT credits FROM workspaces WHERE id = $1")
            .bind(workspace)
            .fetch_optional(&self.pg)
            .await
            .context("failed to load credit balance")?
            .context("workspace no longer exists")
    }

    // once a month the balance is topped up to the plan's allowance, so unused free credits don't pile up
    pub async fn refill_credits(&self, workspace: Uuid, allowance: i64) -> anyhow::Result<i64> {
        let mut tx = self.pg.begin().await.context("failed to refill credits")?;
        let row = sqlx::query(
            "SELECT credits, credits_refilled_for IS NOT NULL AND credits_refilled_for >= DATE_TRUNC('month', NOW())::DATE AS current \
             FROM workspaces WHERE id = $1 FOR UPDATE",
        )
        .bind(workspace)
        .fetch_optional(&mut *tx)
        .await
        .context("failed to refill credits")?
        .context("workspace no longer exists")?;
        let balance: i64 = row.try_get("credits")?;
        if row.try_get::<bool, _>("current")? {
            return Ok(balance);
        }
        let grant = (allowance - balance).max(0);
        let balance = balance + grant;
        sqlx::query(
            "UPDATE workspaces SET credits = $2, credits_refilled_for = DATE_TRUNC('month', NOW())::DATE WHERE id = $1",
        )
        .bind(workspace)
        .bind(balance)
        .execute(&mut *tx)
        .await
        .context("failed to refill credits")?;
        if grant > 0 {
            insert_entry(
                &mut tx,
                workspace,
                grant,
                balance,
                "grant",
                "monthly plan credits",
                None,
            )
            .await?;
        }
        tx.commit().await.context("failed to refill credits")?;
        Ok(balance)
    }

    // the usage is recorded and charged together, so the ledger always matches the usage
    pub async fn charge_usage(&self, usage: &UsageRecord<'_>, credits: i64) -> anyhow::Result<i64> {
        let mut tx = self.pg.begin().await.context("failed to charge usage")?;
        let record = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO usage_records (id, workspace_id, session_id, model, input_tokens, cached_input_tokens, \
             cache_write_tokens, output_tokens, reasoning_tokens, credits, duration_ms) \
             VALUES ($1, $2, $3, $4, $5, $6, $7, $8, $9, $10, $11)",
        )
        .bind(record)
        .bind(usage.workspace_id)
        .bind(usage.session_id)
        .bind(usage.model)
        .bind(usage.input_tokens as i64)
        .bind(usage.cached_input_tokens as i64)
        .bind(usage.cache_write_tokens as i64)
        .bind(usage.output_tokens as i64)
        .bind(usage.reasoning_tokens as i64)
        .bind(credits)
        .bind(usage.duration_ms as i64)
        .execute(&mut *tx)
        .await
        .context("failed to record usage")?;
        let balance: i64 = sqlx::query_scalar(
            "UPDATE workspaces SET credits = credits - $2 WHERE id = $1 RETURNING credits",
        )
        .bind(usage.workspace_id)
        .bind(credits)
        .fetch_one(&mut *tx)
        .await
        .context("failed to charge usage")?;
        if credits > 0 {
            let description = format!("{} response", usage.model);
            insert_entry(
                &mut tx,
                usage.workspace_id,
                -credits,
                balance,
                "usage",
                &description,
                Some(record),
            )
            .await?;
        }
        tx.commit().await.context("failed to charge usage")?;
        Ok(balance)
    }

    pub async fn adjust_credits(
        &self,
        workspace: Uuid,
        amount: i64,
        description: &str,
    ) -> anyhow::Result<Option<i64>> {
        let mut tx = self.pg.begin().await.context("failed to adjust credits")?;
        let balance: Option<i64> = sqlx::query_scalar(
            "UPDATE workspaces SET credits = credits + $2 WHERE id = $1 RETURNING credits",
        )
        .bind(workspace)
        .bind(amount)
        .fetch_optional(&mut *tx)
        .await
        .context("failed to adjust credits")?;
        let Some(balance) = balance else {
            return Ok(None);
        };
        insert_entry(
            &mut tx,
            workspace,
            amount,
            balance,
            "adjustment",
            description,
            None,
        )
        .await?;
        tx.commit().await.context("failed to adjust credits")?;
        Ok(Some(balance))
    }

    pub async fn ledger(
        &self,
        workspace: Uuid,
        limit: i64,
        before: Option<Uuid>,
    ) -> anyhow::Result<Vec<LedgerEntry>> {
        sqlx::query_as(
            "SELECT id, amount, balance, kind, description, EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at \
             FROM credit_ledger WHERE workspace_id = $1 AND ($2::UUID IS NULL OR id < $2) ORDER BY id DESC LIMIT $3",
        )
        .bind(workspace)
        .bind(before)
        .bind(limit)
        .fetch_all(&self.pg)
        .await
        .context("failed to list credit ledger")
    }
}

async fn insert_entry(
    tx: &mut sqlx::PgConnection,
    workspace: Uuid,
    amount: i64,
    balance: i64,
    kind: &str,
    description: &str,
    usage_record: Option<Uuid>,
) -> anyhow::Result<()> {
    sqlx::query(
        "INSERT INTO credit_ledger (id, workspace_id, amount, balance, kind, description, usage_record_id) \
         VALUES ($1, $2, $3, $4, $5, $6, $7)",
    )
    .bind(Uuid::now_v7())
    .bind(workspace)
    .bind(amount)
    .bind(balance)
    .bind(kind)
    .bind(description)
    .bind(usage_record)
    .execute(tx)
    .await
    .context("failed to write credit ledger")?;
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    // needs TREX_DATABASE_URL and TREX_REDIS_URL: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn grants_charges_and_adjusts_credits() {
        dotenvy::dotenv().ok();
        let store = Store::connect(
            &std::env::var("TREX_DATABASE_URL").unwrap(),
            &std::env::var("TREX_REDIS_URL").unwrap(),
        )
        .await
        .unwrap();
        let email = format!("credits-{}@test.trex", Uuid::now_v7());
        let (user, workspace) = store
            .create_user(&email, "Cred", "x")
            .await
            .unwrap()
            .unwrap();
        let workspace = workspace.id;

        assert_eq!(store.credit_balance(workspace).await.unwrap(), 0);
        assert_eq!(store.refill_credits(workspace, 1000).await.unwrap(), 1000);
        assert_eq!(
            store.refill_credits(workspace, 1000).await.unwrap(),
            1000,
            "once a month"
        );

        let session = store
            .create_session(workspace, "gpt-6.1-sol", None, false, None)
            .await
            .unwrap();
        let usage = UsageRecord {
            workspace_id: workspace,
            session_id: session.id,
            model: "gpt-6.1-sol",
            input_tokens: 1000,
            cached_input_tokens: 0,
            cache_write_tokens: 0,
            output_tokens: 100,
            reasoning_tokens: 0,
            duration_ms: 0,
        };
        assert_eq!(store.charge_usage(&usage, 250).await.unwrap(), 750);
        assert_eq!(
            store.charge_usage(&usage, 900).await.unwrap(),
            -150,
            "a run may dip below zero"
        );
        assert_eq!(
            store
                .adjust_credits(workspace, 500, "support top-up")
                .await
                .unwrap(),
            Some(350)
        );
        assert_eq!(
            store
                .adjust_credits(Uuid::now_v7(), 5, "nobody")
                .await
                .unwrap(),
            None
        );

        // a new month tops up to the allowance
        sqlx::query("UPDATE workspaces SET credits_refilled_for = '2000-01-01' WHERE id = $1")
            .bind(workspace)
            .execute(&store.pg)
            .await
            .unwrap();
        assert_eq!(store.refill_credits(workspace, 1000).await.unwrap(), 1000);

        let entries = store.ledger(workspace, 10, None).await.unwrap();
        let summary: Vec<_> = entries
            .iter()
            .map(|entry| (entry.kind.as_str(), entry.amount, entry.balance))
            .collect();
        assert_eq!(
            summary,
            [
                ("grant", 650, 1000),
                ("adjustment", 500, 350),
                ("usage", -900, -150),
                ("usage", -250, 750),
                ("grant", 1000, 1000),
            ]
        );

        sqlx::query("DELETE FROM users WHERE id = $1")
            .bind(user.id)
            .execute(&store.pg)
            .await
            .unwrap();
        sqlx::query("DELETE FROM workspaces WHERE id = $1")
            .bind(workspace)
            .execute(&store.pg)
            .await
            .unwrap();
    }
}
