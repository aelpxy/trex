use std::time::Duration;

use anyhow::Context;
use sqlx::Row;
use uuid::Uuid;

use crate::Store;

// a signed-in browser; the token itself is never stored, only its hash
pub struct UserSession {
    pub id: Uuid,
    pub user: Uuid,
    pub ip: Option<String>,
    pub last_ip: Option<String>,
    pub user_agent: Option<String>,
    pub created_at: i64,
    pub last_used_at: i64,
    pub expires_at: i64,
}

pub struct NewSession<'a> {
    pub user: Uuid,
    pub token_hash: &'a [u8],
    pub lifetime: Duration,
    pub ip: Option<&'a str>,
    pub user_agent: Option<&'a str>,
}

macro_rules! columns {
    () => {
        "id, user_id, ip, last_ip, user_agent, \
         EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at, \
         EXTRACT(EPOCH FROM last_used_at)::BIGINT AS last_used_at, \
         EXTRACT(EPOCH FROM expires_at)::BIGINT AS expires_at"
    };
}

fn session(row: &sqlx::postgres::PgRow) -> sqlx::Result<UserSession> {
    Ok(UserSession {
        id: row.try_get("id")?,
        user: row.try_get("user_id")?,
        ip: row.try_get("ip")?,
        last_ip: row.try_get("last_ip")?,
        user_agent: row.try_get("user_agent")?,
        created_at: row.try_get("created_at")?,
        last_used_at: row.try_get("last_used_at")?,
        expires_at: row.try_get("expires_at")?,
    })
}

impl Store {
    pub async fn create_user_session(&self, new: NewSession<'_>) -> anyhow::Result<Uuid> {
        let id = Uuid::now_v7();
        sqlx::query(
            "INSERT INTO user_sessions (id, user_id, token_hash, expires_at, ip, last_ip, user_agent) \
             VALUES ($1, $2, $3, NOW() + MAKE_INTERVAL(secs => $4), $5, $5, $6)",
        )
        .bind(id)
        .bind(new.user)
        .bind(new.token_hash)
        .bind(new.lifetime.as_secs_f64())
        .bind(new.ip)
        .bind(new.user_agent)
        .execute(&self.pg)
        .await
        .context("failed to create session")?;
        Ok(id)
    }

    // the live session behind a token, noting when and from where it was last used
    pub async fn use_user_session(
        &self,
        token_hash: &[u8],
        ip: Option<&str>,
    ) -> anyhow::Result<Option<UserSession>> {
        let row = sqlx::query(concat!(
            "UPDATE user_sessions SET last_used_at = NOW(), last_ip = COALESCE($2, last_ip) \
             WHERE token_hash = $1 AND expires_at > NOW() \
             AND NOT EXISTS (SELECT 1 FROM users u WHERE u.id = user_sessions.user_id AND u.suspended_at IS NOT NULL) RETURNING ",
            columns!()
        ))
        .bind(token_hash)
        .bind(ip)
        .fetch_optional(&self.pg)
        .await
        .context("failed to check session")?;
        Ok(row.as_ref().map(session).transpose()?)
    }

    // a user's live sessions, most recently used first
    pub async fn user_sessions(&self, user: Uuid) -> anyhow::Result<Vec<UserSession>> {
        let rows = sqlx::query(concat!(
            "SELECT ",
            columns!(),
            " FROM user_sessions WHERE user_id = $1 AND expires_at > NOW() ORDER BY last_used_at DESC"
        ))
        .bind(user)
        .fetch_all(&self.pg)
        .await
        .context("failed to list sessions")?;
        Ok(rows.iter().map(session).collect::<sqlx::Result<_>>()?)
    }

    // false when the user has no such session
    pub async fn delete_user_session(&self, user: Uuid, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM user_sessions WHERE id = $1 AND user_id = $2")
            .bind(id)
            .bind(user)
            .execute(&self.pg)
            .await
            .context("failed to delete session")?;
        Ok(result.rows_affected() > 0)
    }

    // signs the user out everywhere but `keep`, or everywhere; returns how many sessions ended
    pub async fn delete_user_sessions(
        &self,
        user: Uuid,
        keep: Option<Uuid>,
    ) -> anyhow::Result<u64> {
        let result = sqlx::query(
            "DELETE FROM user_sessions WHERE user_id = $1 AND ($2::UUID IS NULL OR id <> $2)",
        )
        .bind(user)
        .bind(keep)
        .execute(&self.pg)
        .await
        .context("failed to delete sessions")?;
        Ok(result.rows_affected())
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    // needs TREX_DATABASE_URL and TREX_REDIS_URL: cargo test -- --ignored
    #[tokio::test]
    #[ignore]
    async fn tracks_and_ends_sessions() {
        dotenvy::dotenv().ok();
        let store = Store::connect(
            &std::env::var("TREX_DATABASE_URL").unwrap(),
            &std::env::var("TREX_REDIS_URL").unwrap(),
        )
        .await
        .unwrap();
        let email = format!("sessions-{}@test.local", Uuid::now_v7());
        let (user, _) = store
            .create_user(&email, "Sam", "hash")
            .await
            .unwrap()
            .unwrap();
        let new = |token: &'static [u8], lifetime| NewSession {
            user: user.id,
            token_hash: token,
            lifetime,
            ip: Some("10.0.0.1"),
            user_agent: Some("Firefox"),
        };
        let a = store
            .create_user_session(new(b"session-a", Duration::from_secs(60)))
            .await
            .unwrap();
        store
            .create_user_session(new(b"session-b", Duration::from_secs(60)))
            .await
            .unwrap();
        store
            .create_user_session(new(b"session-old", Duration::ZERO))
            .await
            .unwrap();

        let used = store
            .use_user_session(b"session-a", Some("10.0.0.2"))
            .await
            .unwrap()
            .unwrap();
        assert_eq!(
            (used.id, used.ip.as_deref(), used.last_ip.as_deref()),
            (a, Some("10.0.0.1"), Some("10.0.0.2"))
        );
        assert!(store.set_user_suspended(user.id, true).await.unwrap());
        assert!(
            store
                .use_user_session(b"session-a", None)
                .await
                .unwrap()
                .is_none(),
            "suspended"
        );
        let (suspended, _) = store.user(user.id).await.unwrap().unwrap();
        assert!(suspended.suspended_at.is_some());
        assert!(store.set_user_suspended(user.id, false).await.unwrap());
        assert!(
            store
                .use_user_session(b"session-a", None)
                .await
                .unwrap()
                .is_some()
        );
        assert!(
            store
                .use_user_session(b"session-old", None)
                .await
                .unwrap()
                .is_none(),
            "expired"
        );
        assert_eq!(store.user_sessions(user.id).await.unwrap().len(), 2);

        assert_eq!(
            store.delete_user_sessions(user.id, Some(a)).await.unwrap(),
            2
        );
        assert!(
            store
                .use_user_session(b"session-b", None)
                .await
                .unwrap()
                .is_none()
        );
        assert!(
            !store.delete_user_session(Uuid::now_v7(), a).await.unwrap(),
            "only the owner's"
        );
        assert!(store.delete_user_session(user.id, a).await.unwrap());
        assert!(store.user_sessions(user.id).await.unwrap().is_empty());
    }
}
