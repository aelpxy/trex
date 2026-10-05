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
    pub async fn set_user_role_by_id(&self, id: Uuid, role: UserRole) -> anyhow::Result<bool> {
        let result = sqlx::query("UPDATE users SET role = $2, updated_at = NOW() WHERE id = $1")
            .bind(id)
            .bind(role.as_str())
            .execute(&self.pg)
            .await
            .context("failed to set user role")?;
        Ok(result.rows_affected() > 0)
    }

    // false when there's no such user; suspending again keeps the first time
    pub async fn set_user_suspended(&self, id: Uuid, suspended: bool) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE users SET suspended_at = CASE WHEN $2 THEN COALESCE(suspended_at, NOW()) END, \
             updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(suspended)
        .execute(&self.pg)
        .await
        .context("failed to suspend user")?;
        Ok(result.rows_affected() > 0)
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
