use anyhow::Context;
use sqlx::{FromRow, Row, postgres::PgRow};
use uuid::Uuid;

use crate::Store;

const PERSONAL_WORKSPACE: &str = "Personal";
const UNIQUE_VIOLATION: &str = "23505";

pub struct User {
    pub id: Uuid,
    pub email: String,
    pub name: String,
    pub role: UserRole,
    pub created_at: i64,
}

// admins run the server: they manage every workspace's credits and plans
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum UserRole {
    User,
    Admin,
}

impl UserRole {
    pub fn as_str(self) -> &'static str {
        match self {
            Self::User => "user",
            Self::Admin => "admin",
        }
    }

    pub(crate) fn parse(value: &str) -> Self {
        if value == "admin" {
            Self::Admin
        } else {
            Self::User
        }
    }
}

// a workspace as admins see it, across all users
pub struct WorkspaceSummary {
    pub id: Uuid,
    pub name: String,
    pub plan: String,
    pub credits: i64,
    pub owner_email: Option<String>,
    // none means every model
    pub allowed_models: Option<Vec<String>>,
    pub created_at: i64,
}

pub struct Workspace {
    pub id: Uuid,
    pub name: String,
    pub plan: String,
    pub role: String,
    pub credits: i64,
    pub created_at: i64,
}

impl FromRow<'_, PgRow> for User {
    fn from_row(row: &PgRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            email: row.try_get("email")?,
            name: row.try_get("name")?,
            role: UserRole::parse(row.try_get::<&str, _>("role")?),
            created_at: row.try_get("created_at")?,
        })
    }
}

impl FromRow<'_, PgRow> for Workspace {
    fn from_row(row: &PgRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            plan: row.try_get("plan")?,
            role: row.try_get("role")?,
            credits: row.try_get("credits")?,
            created_at: row.try_get("created_at")?,
        })
    }
}

impl Store {
    // every user starts with a personal workspace they own; none when the email is taken
    pub async fn create_user(
        &self,
        email: &str,
        name: &str,
        password_hash: &str,
    ) -> anyhow::Result<Option<(User, Workspace)>> {
        let mut tx = self.pg.begin().await.context("failed to create user")?;
        let user: User = match sqlx::query_as(
            "INSERT INTO users (id, email, name, password_hash) VALUES ($1, $2, $3, $4) \
             RETURNING id, email, name, role, EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at",
        )
        .bind(Uuid::now_v7())
        .bind(email)
        .bind(name)
        .bind(password_hash)
        .fetch_one(&mut *tx)
        .await
        {
            Ok(user) => user,
            Err(sqlx::Error::Database(error))
                if error.code().as_deref() == Some(UNIQUE_VIOLATION) =>
            {
                return Ok(None);
            }
            Err(error) => return Err(error).context("failed to create user"),
        };
        let workspace: Workspace = sqlx::query_as(
            "WITH created AS (INSERT INTO workspaces (id, name) VALUES ($1, $2) RETURNING id, name, plan, credits, created_at), \
             member AS (INSERT INTO workspace_members (workspace_id, user_id, role) SELECT id, $3, 'owner' FROM created) \
             SELECT id, name, plan, 'owner' AS role, credits, EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at FROM created",
        )
        .bind(Uuid::now_v7())
        .bind(PERSONAL_WORKSPACE)
        .bind(user.id)
        .fetch_one(&mut *tx)
        .await
        .context("failed to create workspace")?;
        tx.commit().await.context("failed to create user")?;
        Ok(Some((user, workspace)))
    }

    // the user and their password hash, for logging in
    pub async fn user_by_email(&self, email: &str) -> anyhow::Result<Option<(User, String)>> {
        let row = sqlx::query(
            "SELECT id, email, name, role, password_hash, EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at \
             FROM users WHERE LOWER(email) = LOWER($1)",
        )
        .bind(email)
        .fetch_optional(&self.pg)
        .await
        .context("failed to look up user")?;
        row.map(|row| Ok((User::from_row(&row)?, row.try_get("password_hash")?)))
            .transpose()
    }

    pub async fn user(&self, id: Uuid) -> anyhow::Result<Option<(User, String)>> {
        let row = sqlx::query(
            "SELECT id, email, name, role, password_hash, EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at \
             FROM users WHERE id = $1",
        )
        .bind(id)
        .fetch_optional(&self.pg)
        .await
        .context("failed to load user")?;
        row.map(|row| Ok((User::from_row(&row)?, row.try_get("password_hash")?)))
            .transpose()
    }

    // false when no user has that email
    pub async fn set_user_role(&self, email: &str, role: UserRole) -> anyhow::Result<bool> {
        let result = sqlx::query(
            "UPDATE users SET role = $2, updated_at = NOW() WHERE LOWER(email) = LOWER($1)",
        )
        .bind(email)
        .bind(role.as_str())
        .execute(&self.pg)
        .await
        .context("failed to set user role")?;
        Ok(result.rows_affected() > 0)
    }

    // every workspace, newest first, for admins
    pub async fn all_workspaces(&self) -> anyhow::Result<Vec<WorkspaceSummary>> {
        let rows = sqlx::query(
            "SELECT w.id, w.name, w.plan, w.credits, w.allowed_models, EXTRACT(EPOCH FROM w.created_at)::BIGINT AS created_at, \
             (SELECT u.email FROM workspace_members m JOIN users u ON u.id = m.user_id \
              WHERE m.workspace_id = w.id ORDER BY m.role = 'owner' DESC, u.created_at LIMIT 1) AS owner_email \
             FROM workspaces w ORDER BY w.created_at DESC",
        )
        .fetch_all(&self.pg)
        .await
        .context("failed to list workspaces")?;
        rows.iter()
            .map(|row| {
                Ok(WorkspaceSummary {
                    id: row.try_get("id")?,
                    name: row.try_get("name")?,
                    plan: row.try_get("plan")?,
                    credits: row.try_get("credits")?,
                    owner_email: row.try_get("owner_email")?,
                    allowed_models: row.try_get("allowed_models")?,
                    created_at: row.try_get("created_at")?,
                })
            })
            .collect()
    }

    // the models a workspace may use; none means every model
    pub async fn workspace_models(&self, workspace: Uuid) -> anyhow::Result<Option<Vec<String>>> {
        let models: Option<Option<Vec<String>>> =
            sqlx::query_scalar("SELECT allowed_models FROM workspaces WHERE id = $1")
                .bind(workspace)
                .fetch_optional(&self.pg)
                .await
                .context("failed to load workspace models")?;
        Ok(models.flatten())
    }

    // false when there's no such workspace
    pub async fn set_workspace_models(
        &self,
        workspace: Uuid,
        models: Option<&[String]>,
    ) -> anyhow::Result<bool> {
        let result = sqlx::query("UPDATE workspaces SET allowed_models = $2 WHERE id = $1")
            .bind(workspace)
            .bind(models)
            .execute(&self.pg)
            .await
            .context("failed to set workspace models")?;
        Ok(result.rows_affected() > 0)
    }

    pub async fn update_user(
        &self,
        id: Uuid,
        name: Option<&str>,
        password_hash: Option<&str>,
    ) -> anyhow::Result<()> {
        sqlx::query(
            "UPDATE users SET name = COALESCE($2, name), password_hash = COALESCE($3, password_hash), \
             updated_at = NOW() WHERE id = $1",
        )
        .bind(id)
        .bind(name)
        .bind(password_hash)
        .execute(&self.pg)
        .await
        .context("failed to update user")?;
        Ok(())
    }

    pub async fn workspaces(&self, user: Uuid) -> anyhow::Result<Vec<Workspace>> {
        sqlx::query_as(
            "SELECT w.id, w.name, w.plan, m.role, w.credits, EXTRACT(EPOCH FROM w.created_at)::BIGINT AS created_at \
             FROM workspaces w JOIN workspace_members m ON m.workspace_id = w.id \
             WHERE m.user_id = $1 ORDER BY w.created_at",
        )
        .bind(user)
        .fetch_all(&self.pg)
        .await
        .context("failed to list workspaces")
    }

    // the workspace, if the user is a member of it
    pub async fn membership(
        &self,
        user: Uuid,
        workspace: Uuid,
    ) -> anyhow::Result<Option<Workspace>> {
        sqlx::query_as(
            "SELECT w.id, w.name, w.plan, m.role, w.credits, EXTRACT(EPOCH FROM w.created_at)::BIGINT AS created_at \
             FROM workspaces w JOIN workspace_members m ON m.workspace_id = w.id \
             WHERE m.user_id = $1 AND w.id = $2",
        )
        .bind(user)
        .bind(workspace)
        .fetch_optional(&self.pg)
        .await
        .context("failed to check workspace membership")
    }

    pub async fn workspace_plan(&self, workspace: Uuid) -> anyhow::Result<Option<String>> {
        sqlx::query_scalar("SELECT plan FROM workspaces WHERE id = $1")
            .bind(workspace)
            .fetch_optional(&self.pg)
            .await
            .context("failed to load workspace plan")
    }

    pub async fn set_workspace_plan(&self, workspace: Uuid, plan: &str) -> anyhow::Result<bool> {
        let result = sqlx::query("UPDATE workspaces SET plan = $2 WHERE id = $1")
            .bind(workspace)
            .bind(plan)
            .execute(&self.pg)
            .await
            .context("failed to set workspace plan")?;
        Ok(result.rows_affected() > 0)
    }

    // only the hash is stored, so a database leak doesn't leak working tokens
}

#[cfg(test)]
mod tests {
    use super::*;

    // needs TREX_DATABASE_URL and TREX_REDIS_URL: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn users_tokens_and_workspaces() {
        dotenvy::dotenv().ok();
        let store = Store::connect(
            &std::env::var("TREX_DATABASE_URL").unwrap(),
            &std::env::var("TREX_REDIS_URL").unwrap(),
        )
        .await
        .unwrap();
        let email = format!("Sam-{}@Test.trex", Uuid::now_v7());
        let (sam, personal) = store
            .create_user(&email, "Sam", "hash-1")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (
                personal.name.as_str(),
                personal.plan.as_str(),
                personal.role.as_str()
            ),
            ("Personal", "free", "owner")
        );
        assert!(
            store
                .create_user(&email.to_uppercase(), "Copy", "x")
                .await
                .unwrap()
                .is_none(),
            "emails are unique regardless of case"
        );
        let (found, hash) = store
            .user_by_email(&email.to_lowercase())
            .await
            .unwrap()
            .unwrap();
        assert_eq!((found.id, hash.as_str()), (sam.id, "hash-1"));

        store
            .update_user(sam.id, Some("Samantha"), Some("hash-2"))
            .await
            .unwrap();
        let (renamed, hash) = store.user(sam.id).await.unwrap().unwrap();
        assert_eq!(
            (renamed.name.as_str(), hash.as_str()),
            ("Samantha", "hash-2")
        );

        let other_email = format!("eve-{}@test.trex", Uuid::now_v7());
        let (eve, theirs) = store
            .create_user(&other_email, "Eve", "x")
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            store
                .workspaces(sam.id)
                .await
                .unwrap()
                .iter()
                .map(|w| w.id)
                .collect::<Vec<_>>(),
            [personal.id]
        );
        assert!(
            store
                .membership(sam.id, personal.id)
                .await
                .unwrap()
                .is_some()
        );
        assert!(store.membership(sam.id, theirs.id).await.unwrap().is_none());

        for (user, workspace) in [(sam.id, personal.id), (eve.id, theirs.id)] {
            sqlx::query("DELETE FROM users WHERE id = $1")
                .bind(user)
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
}
