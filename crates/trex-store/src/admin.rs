use anyhow::Context;
use sqlx::Row;
use uuid::Uuid;

use sqlx::{Postgres, QueryBuilder};

use crate::{Store, accounts::UserRole};

// what a paged admin list is ordered by; ties go to the newest
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Sort<K> {
    pub key: K,
    pub descending: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserSort {
    Created,
    Name,
    Email,
    LastActive,
    Credits,
}

impl UserSort {
    fn column(self) -> &'static str {
        match self {
            Self::Created => "u.created_at",
            Self::Name => "LOWER(u.name)",
            Self::Email => "LOWER(u.email)",
            Self::LastActive => "last_active_at",
            Self::Credits => "credits",
        }
    }
}

#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WorkspaceSort {
    Created,
    Name,
    Owner,
    Plan,
    Credits,
}

impl WorkspaceSort {
    pub(crate) fn column(self) -> &'static str {
        match self {
            Self::Created => "w.created_at",
            Self::Name => "LOWER(w.name)",
            Self::Owner => "owner_email",
            Self::Plan => "w.plan",
            Self::Credits => "w.credits",
        }
    }
}

// columns only ever come from the sort enums above, never from input
pub(crate) fn push_order(
    query: &mut QueryBuilder<Postgres>,
    column: &'static str,
    descending: bool,
    id: &'static str,
) {
    let direction = if descending {
        "DESC NULLS LAST"
    } else {
        "ASC NULLS LAST"
    };
    query.push(format_args!(" ORDER BY {column} {direction}, {id} DESC"));
}

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
    // the balance of the first workspace they own
    pub credits: Option<i64>,
    pub created_at: i64,
    // when any of their sessions last made a request; none if they have no live session
    pub last_active_at: Option<i64>,
    pub suspended_at: Option<i64>,
}

// a chat whose agent is working right now, in any workspace
pub struct LiveRun {
    pub session: Uuid,
    pub title: Option<String>,
    pub workspace: Uuid,
    pub workspace_name: String,
    pub owner_email: Option<String>,
    pub model: String,
    pub reasoning_effort: Option<String>,
    pub fast: bool,
    pub scheduled: bool,
    // unix seconds; none for runs started before this was recorded
    pub started_at: Option<i64>,
    pub heartbeat_at: Option<i64>,
    pub cancel_requested: bool,
    // spent since the run started
    pub credits: i64,
    pub responses: i64,
}

// what changing a user's role or suspension did
#[derive(Debug, PartialEq, Eq)]
pub enum AdminChange {
    Changed,
    NotFound,
    // it would leave no active admin
    LastAdmin,
}

// a chat that has a sandbox, in any workspace
pub struct SandboxChat {
    pub session: Uuid,
    pub workspace: Uuid,
    pub title: Option<String>,
    pub sandbox: String,
    pub running: bool,
    // unix seconds; idle sandboxes are stopped a while after this
    pub updated_at: i64,
}

pub struct WorkspaceOwner {
    pub workspace: Uuid,
    pub name: String,
    pub owner_email: Option<String>,
}

pub struct DayUsage {
    // unix seconds at midnight utc
    pub day: i64,
    pub credits: i64,
    pub input_tokens: i64,
    pub output_tokens: i64,
    pub responses: i64,
}

// usage of the workspaces an account owns
pub struct AccountUsage {
    pub user: Uuid,
    pub name: String,
    pub email: String,
    pub credits: i64,
    pub tokens: i64,
    pub responses: i64,
}

pub struct UsageReport {
    pub days: Vec<DayUsage>,
    pub models: Vec<ModelUsage>,
    pub accounts: Vec<AccountUsage>,
}

const TOP_MODELS: i64 = 5;
const TOP_ACCOUNTS: i64 = 20;

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
            "WITH spend AS (SELECT workspace_id, SUM(credits) AS credits, SUM(input_tokens + output_tokens) AS tokens, COUNT(*) AS responses \
             FROM usage_records WHERE created_at >= NOW() - MAKE_INTERVAL(days => $1) GROUP BY workspace_id) \
             SELECT o.id, o.name, o.email, SUM(s.credits)::BIGINT AS credits, SUM(s.tokens)::BIGINT AS tokens, SUM(s.responses)::BIGINT AS responses \
             FROM spend s CROSS JOIN LATERAL (SELECT x.id, x.name, x.email FROM workspace_members m JOIN users x ON x.id = m.user_id \
              WHERE m.workspace_id = s.workspace_id ORDER BY m.role = 'owner' DESC, x.created_at LIMIT 1) o \
             GROUP BY o.id, o.name, o.email ORDER BY credits DESC, tokens DESC LIMIT $2",
        )
        .bind(days)
        .bind(TOP_ACCOUNTS)
        .fetch_all(&self.pg)
        .await
        .context("failed to load usage by account")?;
        let accounts = rows
            .iter()
            .map(|row| {
                Ok(AccountUsage {
                    user: row.try_get("id")?,
                    name: row.try_get("name")?,
                    email: row.try_get("email")?,
                    credits: row.try_get("credits")?,
                    tokens: row.try_get("tokens")?,
                    responses: row.try_get("responses")?,
                })
            })
            .collect::<anyhow::Result<Vec<_>>>()?;
        Ok(UsageReport {
            days: days_usage,
            models: self.model_usage(days, i64::MAX).await?,
            accounts,
        })
    }

    // a page of users whose name or email contains `search`
    pub async fn users_page(
        &self,
        search: Option<&str>,
        sort: Sort<UserSort>,
        limit: i64,
        offset: i64,
    ) -> anyhow::Result<Vec<UserSummary>> {
        let pattern = search.map(contains_pattern);
        let mut query = QueryBuilder::new(
            "SELECT u.id, u.email, u.name, u.role, EXTRACT(EPOCH FROM u.created_at)::BIGINT AS created_at, \
             EXTRACT(EPOCH FROM u.suspended_at)::BIGINT AS suspended_at, \
             (SELECT COUNT(*) FROM workspace_members m WHERE m.user_id = u.id) AS workspaces, \
             (SELECT w.credits FROM workspace_members m JOIN workspaces w ON w.id = m.workspace_id \
              WHERE m.user_id = u.id AND m.role = 'owner' ORDER BY w.created_at LIMIT 1) AS credits, \
             (SELECT EXTRACT(EPOCH FROM MAX(t.last_used_at))::BIGINT FROM user_sessions t WHERE t.user_id = u.id) AS last_active_at \
             FROM users u WHERE ",
        );
        query
            .push_bind(pattern.clone())
            .push("::TEXT IS NULL OR u.email ILIKE ")
            .push_bind(pattern.clone())
            .push(" OR u.name ILIKE ")
            .push_bind(pattern);
        push_order(&mut query, sort.key.column(), sort.descending, "u.id");
        query
            .push(" LIMIT ")
            .push_bind(limit)
            .push(" OFFSET ")
            .push_bind(offset);
        let rows = query
            .build()
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
                    credits: row.try_get("credits")?,
                    created_at: row.try_get("created_at")?,
                    last_active_at: row.try_get("last_active_at")?,
                    suspended_at: row.try_get("suspended_at")?,
                })
            })
            .collect()
    }

    pub async fn user_count(&self, search: Option<&str>) -> anyhow::Result<i64> {
        sqlx::query_scalar(
            "SELECT COUNT(*) FROM users u WHERE $1::TEXT IS NULL OR u.email ILIKE $1 OR u.name ILIKE $1",
        )
        .bind(search.map(contains_pattern))
        .fetch_one(&self.pg)
        .await
        .context("failed to count users")
    }

    // false when there's no such user
    pub async fn set_user_role_by_id(
        &self,
        id: Uuid,
        role: UserRole,
    ) -> anyhow::Result<AdminChange> {
        // the active admins are locked, so two admins demoting each other at once can't both succeed
        let result = sqlx::query(
            "WITH active AS (SELECT id FROM users WHERE role = 'admin' AND suspended_at IS NULL FOR UPDATE) \
             UPDATE users SET role = $2, updated_at = NOW() WHERE id = $1 \
             AND ($2 = 'admin' OR NOT EXISTS (SELECT 1 FROM active WHERE id = $1) OR EXISTS (SELECT 1 FROM active WHERE id <> $1))",
        )
        .bind(id)
        .bind(role.as_str())
        .execute(&self.pg)
        .await
        .context("failed to set user role")?;
        self.admin_change(id, result.rows_affected()).await
    }

    async fn admin_change(&self, id: Uuid, changed: u64) -> anyhow::Result<AdminChange> {
        if changed > 0 {
            return Ok(AdminChange::Changed);
        }
        let exists: bool = sqlx::query_scalar("SELECT EXISTS (SELECT 1 FROM users WHERE id = $1)")
            .bind(id)
            .fetch_one(&self.pg)
            .await
            .context("failed to look up user")?;
        Ok(if exists {
            AdminChange::LastAdmin
        } else {
            AdminChange::NotFound
        })
    }

    pub async fn sandbox_chats(&self) -> anyhow::Result<Vec<SandboxChat>> {
        let rows = sqlx::query(
            "SELECT id, workspace_id, title, sandbox, status = 'running' AS running, \
             EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at FROM sessions WHERE sandbox IS NOT NULL",
        )
        .fetch_all(&self.pg)
        .await
        .context("failed to list sandbox chats")?;
        rows.iter()
            .map(|row| {
                Ok(SandboxChat {
                    session: row.try_get("id")?,
                    workspace: row.try_get("workspace_id")?,
                    title: row.try_get("title")?,
                    sandbox: row.try_get("sandbox")?,
                    running: row.try_get("running")?,
                    updated_at: row.try_get("updated_at")?,
                })
            })
            .collect()
    }

    pub async fn workspace_owners(&self) -> anyhow::Result<Vec<WorkspaceOwner>> {
        let rows = sqlx::query(
            "SELECT w.id, w.name, (SELECT u.email FROM workspace_members m JOIN users u ON u.id = m.user_id \
             WHERE m.workspace_id = w.id ORDER BY m.role = 'owner' DESC, u.created_at LIMIT 1) AS owner_email \
             FROM workspaces w",
        )
        .fetch_all(&self.pg)
        .await
        .context("failed to list workspaces")?;
        rows.iter()
            .map(|row| {
                Ok(WorkspaceOwner {
                    workspace: row.try_get("id")?,
                    name: row.try_get("name")?,
                    owner_email: row.try_get("owner_email")?,
                })
            })
            .collect()
    }

    // whether a chat in `workspace` is running with this sandbox, which an admin mustn't stop under it
    pub async fn sandbox_in_use(&self, workspace: Uuid, sandbox: &str) -> anyhow::Result<bool> {
        sqlx::query_scalar(
            "SELECT EXISTS (SELECT 1 FROM sessions WHERE workspace_id = $1 AND sandbox = $2 AND status = 'running')",
        )
        .bind(workspace)
        .bind(sandbox)
        .fetch_one(&self.pg)
        .await
        .context("failed to check sandbox use")
    }

    // an admin stopped it, so the idle sweep has nothing left to do for its chat
    pub async fn mark_sandbox_stopped(&self, workspace: Uuid, sandbox: &str) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE sessions SET sandbox_stopped = TRUE WHERE workspace_id = $1 AND sandbox = $2",
        )
        .bind(workspace)
        .bind(sandbox)
        .execute(&self.pg)
        .await
        .context("failed to mark sandbox stopped")?;
        Ok(())
    }

    // oldest first, so the longest running are on top
    pub async fn live_runs(&self) -> anyhow::Result<Vec<LiveRun>> {
        let rows = sqlx::query(
            "SELECT s.id, s.title, s.workspace_id, w.name AS workspace_name, s.model, s.reasoning_effort, s.fast, \
             s.scheduled_task_id IS NOT NULL AS scheduled, s.run_cancel_requested, \
             EXTRACT(EPOCH FROM s.run_started_at)::BIGINT AS started_at, \
             EXTRACT(EPOCH FROM s.run_heartbeat_at)::BIGINT AS heartbeat_at, \
             (SELECT u.email FROM workspace_members m JOIN users u ON u.id = m.user_id \
              WHERE m.workspace_id = s.workspace_id ORDER BY m.role = 'owner' DESC, u.created_at LIMIT 1) AS owner_email, \
             (SELECT COALESCE(SUM(r.credits), 0)::BIGINT FROM usage_records r \
              WHERE r.session_id = s.id AND r.created_at >= s.run_started_at) AS credits, \
             (SELECT COUNT(*) FROM usage_records r WHERE r.session_id = s.id AND r.created_at >= s.run_started_at) AS responses \
             FROM sessions s JOIN workspaces w ON w.id = s.workspace_id \
             WHERE s.status = 'running' ORDER BY s.run_started_at NULLS FIRST, s.id",
        )
        .fetch_all(&self.pg)
        .await
        .context("failed to list live runs")?;
        rows.iter()
            .map(|row| {
                Ok(LiveRun {
                    session: row.try_get("id")?,
                    title: row.try_get("title")?,
                    workspace: row.try_get("workspace_id")?,
                    workspace_name: row.try_get("workspace_name")?,
                    owner_email: row.try_get("owner_email")?,
                    model: row.try_get("model")?,
                    reasoning_effort: row.try_get("reasoning_effort")?,
                    fast: row.try_get("fast")?,
                    scheduled: row.try_get("scheduled")?,
                    started_at: row.try_get("started_at")?,
                    heartbeat_at: row.try_get("heartbeat_at")?,
                    cancel_requested: row.try_get("run_cancel_requested")?,
                    credits: row.try_get("credits")?,
                    responses: row.try_get("responses")?,
                })
            })
            .collect()
    }

    // false when there's no such user; suspending again keeps the first time
    pub async fn set_user_suspended(
        &self,
        id: Uuid,
        suspended: bool,
    ) -> anyhow::Result<AdminChange> {
        let result = sqlx::query(
            "WITH active AS (SELECT id FROM users WHERE role = 'admin' AND suspended_at IS NULL FOR UPDATE) \
             UPDATE users SET suspended_at = CASE WHEN $2 THEN COALESCE(suspended_at, NOW()) END, \
             updated_at = NOW() WHERE id = $1 \
             AND (NOT $2 OR NOT EXISTS (SELECT 1 FROM active WHERE id = $1) OR EXISTS (SELECT 1 FROM active WHERE id <> $1))",
        )
        .bind(id)
        .bind(suspended)
        .execute(&self.pg)
        .await
        .context("failed to suspend user")?;
        self.admin_change(id, result.rows_affected()).await
    }
}

// an ILIKE pattern matching text that contains `search` literally
pub(crate) fn contains_pattern(search: &str) -> String {
    let escaped = search
        .replace('\\', "\\\\")
        .replace('%', "\\%")
        .replace('_', "\\_");
    format!("%{escaped}%")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn searches_match_text_literally() {
        assert_eq!(contains_pattern("ada"), "%ada%");
        assert_eq!(contains_pattern(r"50%_a\b"), r"%50\%\_a\\b%");
    }

    // needs TREX_DATABASE_URL and TREX_REDIS_URL: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn pages_and_searches_users_and_workspaces() {
        dotenvy::dotenv().ok();
        let store = Store::connect(
            &std::env::var("TREX_DATABASE_URL").unwrap(),
            &std::env::var("TREX_REDIS_URL").unwrap(),
        )
        .await
        .unwrap();
        let tag = Uuid::now_v7().simple().to_string();
        let mut created = Vec::new();
        for name in ["Ann", "Bob", "Cy"] {
            let email = format!("{name}-{tag}@test.trex");
            let (user, workspace) = store.create_user(&email, name, "x").await.unwrap().unwrap();
            created.push((user.id, workspace.id, email));
        }

        assert_eq!(store.user_count(Some(&tag)).await.unwrap(), 3);
        let newest = Sort {
            key: UserSort::Created,
            descending: true,
        };
        let first = store.users_page(Some(&tag), newest, 2, 0).await.unwrap();
        let second = store.users_page(Some(&tag), newest, 2, 2).await.unwrap();
        let names: Vec<_> = first
            .iter()
            .chain(&second)
            .map(|u| u.name.as_str())
            .collect();
        assert_eq!(
            names,
            ["Cy", "Bob", "Ann"],
            "newest first, split across pages"
        );
        assert_eq!(second[0].credits, Some(0));
        let by_name = Sort {
            key: UserSort::Name,
            descending: false,
        };
        let sorted = store.users_page(Some(&tag), by_name, 20, 0).await.unwrap();
        let names: Vec<_> = sorted.iter().map(|u| u.name.as_str()).collect();
        assert_eq!(names, ["Ann", "Bob", "Cy"]);
        assert_eq!(
            store.user_count(Some(&format!("ann-{tag}"))).await.unwrap(),
            1
        );
        assert_eq!(
            store.user_count(Some("%")).await.unwrap(),
            0,
            "% is literal"
        );

        let ann = created[0].0;
        assert_eq!(
            store
                .set_user_role_by_id(ann, UserRole::Admin)
                .await
                .unwrap(),
            AdminChange::Changed
        );
        assert_eq!(
            store.set_user_suspended(ann, true).await.unwrap(),
            AdminChange::Changed
        );
        assert_eq!(
            store.set_user_suspended(ann, false).await.unwrap(),
            AdminChange::Changed
        );
        assert_eq!(
            store
                .set_user_role_by_id(ann, UserRole::User)
                .await
                .unwrap(),
            AdminChange::Changed
        );
        assert_eq!(
            store
                .set_user_role_by_id(Uuid::now_v7(), UserRole::User)
                .await
                .unwrap(),
            AdminChange::NotFound
        );
        store.live_runs().await.unwrap();
        store.sandbox_chats().await.unwrap();
        assert!(store.workspace_owners().await.unwrap().len() >= 3);
        assert!(!store.sandbox_in_use(created[0].1, "none").await.unwrap());
        store
            .mark_sandbox_stopped(created[0].1, "none")
            .await
            .unwrap();
        let report = store.usage_report(30).await.unwrap();
        assert!(report.accounts.len() as i64 <= TOP_ACCOUNTS);

        assert_eq!(store.workspace_count(Some(&tag), None).await.unwrap(), 3);
        let by_owner = Sort {
            key: WorkspaceSort::Owner,
            descending: true,
        };
        let (_, bob_workspace, bob_email) = &created[1];
        let found = store
            .workspaces_page(None, Some(*bob_workspace), by_owner, 20, 0)
            .await
            .unwrap();
        assert_eq!(found.len(), 1);
        assert_eq!(found[0].owner_email.as_deref(), Some(bob_email.as_str()));
        let page = store
            .workspaces_page(Some(&tag), None, by_owner, 2, 2)
            .await
            .unwrap();
        assert_eq!(page.len(), 1);
        assert_eq!(page[0].owner_email.as_deref(), Some(created[0].2.as_str()));

        for (user, workspace, _) in &created {
            assert_eq!(store.sole_workspaces(*user).await.unwrap(), [*workspace]);
            assert!(store.delete_user(*user, &[*workspace]).await.unwrap());
            assert!(!store.delete_user(*user, &[*workspace]).await.unwrap());
        }
        assert_eq!(store.user_count(Some(&tag)).await.unwrap(), 0);
        assert_eq!(store.workspace_count(Some(&tag), None).await.unwrap(), 0);
    }
}
