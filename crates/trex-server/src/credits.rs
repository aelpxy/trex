use std::collections::HashMap;

use anyhow::Context;
use futures::future::BoxFuture;
use serde::Deserialize;
use trex_harness::agent::Budget;
use trex_store::{Store, sessions::UsageRecord};
use uuid::Uuid;

use crate::api::{AppState, error::ApiError};

// billing plans from trex.toml; credits are only enforced when at least one is configured
#[derive(Default)]
pub struct Plans {
    plans: HashMap<String, Plan>,
}

#[derive(Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Plan {
    pub name: String,
    pub monthly_credits: i64,
}

#[derive(Deserialize)]
struct PlansFile {
    #[serde(default)]
    plans: HashMap<String, Plan>,
}

impl Plans {
    pub fn from_toml(raw: &str) -> anyhow::Result<Self> {
        let file: PlansFile = toml::from_str(raw)?;
        if let Some((id, _)) = file.plans.iter().find(|(_, plan)| plan.monthly_credits < 0) {
            anyhow::bail!("plan {id}: monthly_credits must not be negative");
        }
        Ok(Self { plans: file.plans })
    }

    pub fn enforced(&self) -> bool {
        !self.plans.is_empty()
    }

    pub fn get(&self, id: &str) -> Option<&Plan> {
        self.plans.get(id)
    }

    pub fn ids(&self) -> impl Iterator<Item = &str> {
        self.plans.keys().map(String::as_str)
    }
}

// the balance after this month's refill; refuses when nothing is left
pub async fn require(state: &AppState, workspace: Uuid) -> Result<i64, ApiError> {
    if !state.plans.enforced() {
        return Ok(state.store.credit_balance(workspace).await?);
    }
    let balance = refilled_balance(state, workspace).await?;
    if balance <= 0 {
        return Err(ApiError::InsufficientCredits(
            "this workspace has no credits left".into(),
        ));
    }
    Ok(balance)
}

pub async fn refilled_balance(state: &AppState, workspace: Uuid) -> anyhow::Result<i64> {
    let plan = state
        .store
        .workspace_plan(workspace)
        .await?
        .context("workspace no longer exists")?;
    let allowance = match state.plans.get(&plan) {
        Some(plan) => plan.monthly_credits,
        None => {
            tracing::warn!(workspace = %workspace, plan, "workspace has an unknown plan");
            0
        }
    };
    state.store.refill_credits(workspace, allowance).await
}

// usage is always charged, so balances are right once enforcement is turned on; returns the credits charged
pub async fn charge(state: &AppState, usage: &UsageRecord<'_>, fast: bool) -> anyhow::Result<i64> {
    let credits = state.models.get(usage.model).map_or(0, |model| {
        model.credits(
            usage.input_tokens,
            usage.cached_input_tokens,
            usage.output_tokens,
            fast,
        )
    });
    state.store.charge_usage(usage, credits).await?;
    Ok(credits)
}

pub struct WorkspaceBudget<'a> {
    pub store: &'a Store,
    pub workspace: Uuid,
    pub enforced: bool,
}

impl Budget for WorkspaceBudget<'_> {
    fn exhausted(&self) -> BoxFuture<'_, anyhow::Result<bool>> {
        Box::pin(async move {
            if !self.enforced {
                return Ok(false);
            }
            Ok(self.store.credit_balance(self.workspace).await? <= 0)
        })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_plans_from_the_catalog() {
        let plans = Plans::from_toml(
            "[plans.free]\nname = \"Free\"\nmonthly_credits = 1000\n\n[plans.pro]\nname = \"Pro\"\nmonthly_credits = 50000\n",
        )
        .unwrap();
        assert!(plans.enforced());
        assert_eq!(
            plans.get("pro").map(|plan| plan.monthly_credits),
            Some(50_000)
        );
        assert!(!Plans::from_toml("").unwrap().enforced());
        assert!(Plans::from_toml("[plans.bad]\nname = \"Bad\"\nmonthly_credits = -1\n").is_err());
    }
}
