use anyhow::Context;
use sqlx::Row;
use uuid::Uuid;

use crate::{Store, accounts::UserRole};

// server-wide numbers for the admin overview; usage covers responses since midnight utc and the
// last 30 days
pub struct Overview {
    pub users: i64,
    pub admins: i64,
    pub workspaces: i64,
    pub chats: i64,
    pub running: i64,
    pub spend_today: i64,
    pub tokens_today: i64,
    pub spend_month: i64,
    pub tokens_month: i64,
    pub top_models: Vec<ModelUsage>,
}

pub struct ModelUsage {
    pub model: String,
    pub credits: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub responses: i64,
}

pub struct UserSummary {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: UserRole,
    pub workspaces: i64,
    pub created_at: i64,
    // when any of their sessions last made a request; none if they have no live session
    pub last_active_at: Option<i64>,
}

pub struct DayUsage {
    // unix seconds at midnight utc
    pub day: i64,
    pub credits: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub responses: i64,
}

pub struct WorkspaceUsage {
    pub workspace: Uuid,
    pub name: String,
    pub credits: i64,
    pub tokens: i64,
    pub responses: i64,
}

pub struct UsageReport {
    pub days: Vec<DayUsage>,
    pub models: Vec<ModelUsage>,
    pub workspaces: Vec<WorkspaceUsage>,
}

const TOP_MODELS: i64 = 5;
const TOP_WORKSPACES: i64 = 20;

impl Store {
    pub async fn overview(&self) -> anyhow::Result<Overview> {
        let row = sqlx::query(
            "SELECT (SELECT COUNT(*) FROM users) AS users, \
             (SELECT COUNT(*) FROM users WHERE role = 'admin') AS admins, \
             (SELECT COUNT(*) FROM workspaces) AS workspaces, \
             (SELECT COUNT(*) FROM sessions) AS chats, \
             (SELECT COUNT(*) FROM sessions WHERE status = 'running') AS running, \
             (SELECT COALESCE(SUM(credits), 0)::BIGINT FROM usage_records WHERE created_at >= DATE_TRUNC('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC') AS spend_today, \
             (SELECT COALESCE(SUM(input_tokens + output_tokens), 0)::BIGINT FROM usage_records WHERE created_at >= DATE_TRUNC('day', NOW() AT TIME ZONE 'UTC') AT TIME ZONE 'UTC') AS tokens_today, \
             (SELECT COALESCE(SUM(credits), 0)::BIGINT FROM usage_records WHERE created_at >= NOW() - INTERVAL '30 days') AS spend_month, \
             (SELECT COALESCE(SUM(input_tokens + output_tokens), 0)::BIGINT FROM usage_records WHERE created_at >= NOW() - INTERVAL '30 days') AS tokens_month",
        )
        .fetch_one(&self.pg)
        .await
        .context("failed to load the overview")?;
        Ok(Overview {
            users: row.try_get("users")?,
            admins: row.try_get("admins")?,
            workspaces: row.try_get("workspaces")?,
            chats: row.try_get("chats")?,
            running: row.try_get("running")?,
            spend_today: row.try_get("spend_today")?,
            tokens_today: row.try_get("tokens_today")?,
            spend_month: row.try_get("spend_month")?,
            tokens_month: row.try_get("tokens_month")?,
            top_models: self.model_usage(30, TOP_MODELS).await?,
        })
    }

    async fn model_usage(&self, days: i32, limit: i64) -> anyhow::Result<Vec<ModelUsage>> {
        let rows = sqlx::query(
            "SELECT model, SUM(credits)::BIGINT AS credits, SUM(input_tokens)::BIGINT AS input_tokens, \
             SUM(output_tokens)::BIGINT AS output_tokens, COUNT(*) AS responses \
             FROM usage_records WHERE created_at >= NOW() - MAKE_INTERVAL(days => $1) \
             GROUP BY model ORDER BY SUM(credits) DESC, SUM(input_tokens + output_tokens) DESC LIMIT $2",
        )
        .bind(days)
        .bind(limit)
        .fetch_all(&self.pg)
        .await
        .context("failed to load usage by model")?;
        rows.iter()
            .map(|row| {
                Ok(ModelUsage {
                    model: row.try_get("model")?,
                    credits: row.try_get("credits")?,
                    input_tokens: row.try_get("input_tokens")?,
                    output_tokens: row.try_get("output_tokens")?,
                    responses: row.try_get("responses")?,
                })
            })
            .collect()
    }

    pub async fn usage_report(&self, days: i32) -> anyhow::Result<UsageReport> {
        let rows = sqlx::query(
            "SELECT EXTRACT(EPOCH FROM DATE_TRUNC('day', created_at AT TIME ZONE 'UTC'))::BIGINT AS day, \
             SUM(credits)::BIGINT AS credits, SUM(input_tokens)::BIGINT AS input_tokens, \
             SUM(output_tokens)::BIGINT AS output_tokens, COUNT(*) AS responses \
             FROM usage_records WHERE created_at >= NOW() - MAKE_INTERVAL(days => $1) GROUP BY 1 ORDER BY 1",
        )
        .bind(days)
        .fetch_all(&self.pg)
        .await
        .context("failed to load usage by day")?;
        let days_usage = rows
            .iter()
            .map(|row| {
                Ok(DayUsage {
                    day: row.try_get("day")?,
                    credits: row.try_get("credits")?,
                    input_tokens: row.try_get("input_tokens")?,
                    output_tokens: row.try_get("output_tokens")?,
                    responses: row.try_get("responses")?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        let rows = sqlx::query(
            "SELECT u.workspace_id, w.name, SUM(u.credits)::BIGINT AS credits, \
             SUM(u.input_tokens + u.output_tokens)::BIGINT AS tokens, COUNT(*) AS responses \
             FROM usage_records u JOIN workspaces w ON w.id = u.workspace_id \
             WHERE u.created_at >= NOW() - MAKE_INTERVAL(days => $1) \
             GROUP BY u.workspace_id, w.name ORDER BY SUM(u.credits) DESC, SUM(u.input_tokens + u.output_tokens) DESC LIMIT $2",
        )
        .bind(days)
        .bind(TOP_WORKSPACES)
        .fetch_all(&self.pg)
        .await
        .context("failed to load usage by workspace")?;
        let workspaces = rows
            .iter()
            .map(|row| {
                Ok(WorkspaceUsage {
                    workspace: row.try_get("workspace_id")?,
                    name: row.try_get("name")?,
                    credits: row.try_get("credits")?,
                    tokens: row.try_get("tokens")?,
                    responses: row.try_get("responses")?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(UsageReport {
            days: days_usage,
            models: self.model_usage(days, i64::MAX).await?,
            workspaces,
        })
    }

    pub async fn all_users(&self) -> anyhow::Result<Vec<UserSummary>> {
        let rows = sqlx::query(
            "SELECT u.id, u.email, u.name, u.role, EXTRACT(EPOCH FROM u.created_at)::BIGINT AS created_at, \
             (SELECT COUNT(*) FROM workspace_members m WHERE m.user_id = u.id) AS workspaces, \
             (SELECT EXTRACT(EPOCH FROM MAX(t.last_used_at))::BIGINT FROM user_sessions t WHERE t.user_id = u.id) AS last_active_at \
             FROM users u ORDER BY u.created_at DESC",
        )
        .fetch_all(&self.pg)
        .await
        .context("failed to list users")?;
        rows.iter()
            .map(|row| {
                Ok(UserSummary {
                    id: row.try_get("id")?,
                    email: row.try_get("email")?,
                    name: row.try_get("name")?,
                    role: if row.try_get::<&str, _>("role")? == "admin" {
                        UserRole::Admin
                    } else {
                        UserRole::User
                    },
                    workspaces: row.try_get("workspaces")?,
                    created_at: row.try_get("created_at")?,
                    last_active_at: row.try_get("last_active_at")?,
                })
            })
            .collect()
    }

    // false when there's no such user
    pub async fn set_user_role_by_id(&self, id: Uuid, role: UserRole) -> anyhow::Result<bool> {
        let result = sqlx::query("UPDATE users SET role = $2, updated_at = NOW() WHERE id = $1")
            .bind(id)
            .bind(role.as_str())
            .execute(&self.pg)
            .await
            .context("failed to set user role")?;
        Ok(result.rows_affected() > 0)
    }
}
