pub mod library;
pub mod logs;
pub mod runs;
pub mod usage;
pub mod users;
pub mod workspaces;

use std::sync::Arc;

use axum::{extract::FromRequestParts, http::request::Parts};
use serde::Deserialize;
use trex_store::{
    accounts::UserRole,
    admin::{Sort, UserSort, WorkspaceSort},
};
use utoipa::IntoParams;

use crate::api::{AppState, auth::Account, error::ApiError};

const DEFAULT_PAGE_SIZE: i64 = 20;
const MAX_PAGE_SIZE: i64 = 100;

#[derive(Deserialize, IntoParams)]
#[into_params(parameter_in = Query)]
pub struct PageQuery {
    /// Page size, 1 to 100.
    #[param(default = 20, minimum = 1, maximum = 100)]
    limit: Option<i64>,
    /// A page number, from 1.
    #[param(default = 1, minimum = 1)]
    page: Option<i64>,
    /// Text to search for.
    #[param(example = "ada@example.com")]
    q: Option<String>,
    /// A column to order by, descending with a leading `-`; newest first by default.
    #[param(example = "-credits")]
    sort: Option<String>,
}

impl PageQuery {
    // the page size and how many items come before the page
    fn window(&self) -> Result<(i64, i64), ApiError> {
        let limit = self.limit.unwrap_or(DEFAULT_PAGE_SIZE);
        if !(1..=MAX_PAGE_SIZE).contains(&limit) {
            return Err(ApiError::invalid(
                format!("limit must be between 1 and {MAX_PAGE_SIZE}"),
                "limit",
            ));
        }
        let page = self.page.unwrap_or(1);
        if page < 1 {
            return Err(ApiError::invalid("page starts at 1", "page"));
        }
        let offset = (page - 1)
            .checked_mul(limit)
            .ok_or_else(|| ApiError::invalid("page is too large", "page"))?;
        Ok((limit, offset))
    }

    fn search(&self) -> Option<&str> {
        self.q.as_deref().map(str::trim).filter(|q| !q.is_empty())
    }

    // `keys` names the sortable columns; the first is the newest-first default
    fn sort<K: Copy>(&self, keys: &[(&str, K)]) -> Result<Sort<K>, ApiError> {
        let Some(raw) = self.sort.as_deref().filter(|raw| !raw.is_empty()) else {
            return Ok(Sort {
                key: keys[0].1,
                descending: true,
            });
        };
        let (name, descending) = match raw.strip_prefix('-') {
            Some(name) => (name, true),
            None => (raw, false),
        };
        keys.iter()
            .find(|(candidate, _)| *candidate == name)
            .map(|&(_, key)| Sort { key, descending })
            .ok_or_else(|| {
                let names: Vec<_> = keys.iter().map(|(name, _)| *name).collect();
                ApiError::invalid(
                    format!(
                        "sort must be one of {}, optionally with a leading -",
                        names.join(", ")
                    ),
                    "sort",
                )
            })
    }
}

const USER_SORTS: &[(&str, UserSort)] = &[
    ("created_at", UserSort::Created),
    ("name", UserSort::Name),
    ("email", UserSort::Email),
    ("last_active_at", UserSort::LastActive),
    ("credits", UserSort::Credits),
];

const WORKSPACE_SORTS: &[(&str, WorkspaceSort)] = &[
    ("created_at", WorkspaceSort::Created),
    ("name", WorkspaceSort::Name),
    ("owner_email", WorkspaceSort::Owner),
    ("plan", WorkspaceSort::Plan),
    ("credits", WorkspaceSort::Credits),
];

// a signed-in user with the admin role; admins are made with `trex admin grant <email>`
pub struct Admin {
    pub user: uuid::Uuid,
    pub session: uuid::Uuid,
}

impl FromRequestParts<Arc<AppState>> for Admin {
    type Rejection = ApiError;

    async fn from_request_parts(
        parts: &mut Parts,
        state: &Arc<AppState>,
    ) -> Result<Self, Self::Rejection> {
        let account = Account::from_request_parts(parts, state).await?;
        let role = state
            .store
            .user(account.user)
            .await?
            .map(|(user, _)| user.role);
        if role != Some(UserRole::Admin) {
            return Err(ApiError::Permission("only admins can do this".into()));
        }
        Ok(Self {
            user: account.user,
            session: account.session,
        })
    }
}

fn not_found(id: &str) -> ApiError {
    ApiError::NotFound(format!("no workspace {id}"))
}

#[cfg(test)]
mod tests {
    use super::*;

    fn query(sort: Option<&str>) -> PageQuery {
        PageQuery {
            limit: None,
            page: None,
            q: None,
            sort: sort.map(str::to_owned),
        }
    }

    #[test]
    fn reads_the_sort() {
        let newest = Sort {
            key: UserSort::Created,
            descending: true,
        };
        assert_eq!(query(None).sort(USER_SORTS).ok().unwrap(), newest);
        assert_eq!(
            query(Some("email")).sort(USER_SORTS).ok().unwrap(),
            Sort {
                key: UserSort::Email,
                descending: false
            }
        );
        assert_eq!(
            query(Some("-credits")).sort(WORKSPACE_SORTS).ok().unwrap(),
            Sort {
                key: WorkspaceSort::Credits,
                descending: true
            }
        );
        assert!(query(Some("password")).sort(USER_SORTS).is_err());
        assert_eq!(query(None).window().ok().unwrap(), (DEFAULT_PAGE_SIZE, 0));
    }
}
