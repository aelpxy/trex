use anyhow::Context;
use sqlx::{FromRow, Row, postgres::PgRow};
use uuid::Uuid;

use crate::Store;

macro_rules! columns {
    () => {
        "id, name, instructions, EXTRACT(EPOCH FROM created_at)::BIGINT AS created_at, \
         EXTRACT(EPOCH FROM updated_at)::BIGINT AS updated_at"
    };
}

pub struct Project {
    pub id: Uuid,
    pub name: String,
    pub instructions: Option<String>,
    pub created_at: i64,
    pub updated_at: i64,
}

impl FromRow<'_, PgRow> for Project {
    fn from_row(row: &PgRow) -> sqlx::Result<Self> {
        Ok(Self {
            id: row.try_get("id")?,
            name: row.try_get("name")?,
            instructions: row.try_get("instructions")?,
            created_at: row.try_get("created_at")?,
            updated_at: row.try_get("updated_at")?,
        })
    }
}

impl Store {
    pub async fn create_project(
        &self,
        workspace: Uuid,
        name: &str,
        instructions: Option<&str>,
    ) -> anyhow::Result<Project> {
        let sql = concat!(
            "INSERT INTO projects (id, workspace_id, name, instructions) VALUES ($1, $2, $3, $4) RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(Uuid::now_v7())
            .bind(workspace)
            .bind(name)
            .bind(instructions)
            .fetch_one(&self.pg)
            .await
            .context("failed to create project")
    }

    pub async fn project(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<Option<Project>> {
        let sql = concat!(
            "SELECT ",
            columns!(),
            " FROM projects WHERE id = $1 AND workspace_id = $2"
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(workspace)
            .fetch_optional(&self.pg)
            .await
            .context("failed to load project")
    }

    pub async fn projects(
        &self,
        workspace: Uuid,
        limit: i64,
        before: Option<Uuid>,
    ) -> anyhow::Result<Vec<Project>> {
        let sql = concat!(
            "SELECT ",
            columns!(),
            " FROM projects WHERE workspace_id = $1 AND ($2::UUID IS NULL OR id < $2) ORDER BY id DESC LIMIT $3"
        );
        sqlx::query_as(sql)
            .bind(workspace)
            .bind(before)
            .bind(limit)
            .fetch_all(&self.pg)
            .await
            .context("failed to list projects")
    }

    // `instructions` of Some(None) clears them
    pub async fn update_project(
        &self,
        workspace: Uuid,
        id: Uuid,
        name: Option<&str>,
        instructions: Option<Option<&str>>,
    ) -> anyhow::Result<Option<Project>> {
        let sql = concat!(
            "UPDATE projects SET name = COALESCE($3, name), \
             instructions = CASE WHEN $4 THEN $5 ELSE instructions END, updated_at = NOW() \
             WHERE id = $1 AND workspace_id = $2 RETURNING ",
            columns!()
        );
        sqlx::query_as(sql)
            .bind(id)
            .bind(workspace)
            .bind(name)
            .bind(instructions.is_some())
            .bind(instructions.flatten())
            .fetch_optional(&self.pg)
            .await
            .context("failed to update project")
    }

    // its chats are kept and move out of the project
    pub async fn delete_project(&self, workspace: Uuid, id: Uuid) -> anyhow::Result<bool> {
        let result = sqlx::query("DELETE FROM projects WHERE id = $1 AND workspace_id = $2")
            .bind(id)
            .bind(workspace)
            .execute(&self.pg)
            .await
            .context("failed to delete project")?;
        Ok(result.rows_affected() > 0)
    }
}
