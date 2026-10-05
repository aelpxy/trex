use anyhow::{Context, bail};
use async_openai::types::responses::FunctionTool;
use futures::future::BoxFuture;
use serde::Deserialize;
use serde_json::{Value, json};

use super::{Tool, ToolContext};
use crate::event::{Event, PlanStep, StepStatus};

const MAX_STEPS: usize = 20;

pub struct UpdatePlan;

#[derive(Deserialize)]
struct Args {
    explanation: Option<String>,
    plan: Vec<PlanStep>,
}

impl Tool for UpdatePlan {
    fn definition(&self) -> FunctionTool {
        FunctionTool {
            name: "update_plan".into(),
            description: Some(
                "Keep a short plan for a task with several steps, shown to the user as a checklist. Send the whole \
                 plan every time: each step is pending, in_progress or completed, with at most one in_progress. \
                 Update it as you finish steps or when the plan changes. Skip it for simple tasks."
                    .into(),
            ),
            parameters: Some(json!({
                "type": "object",
                "properties": {
                    "explanation": {"type": ["string", "null"], "description": "Why the plan changed, if it did."},
                    "plan": {
                        "type": "array",
                        "items": {
                            "type": "object",
                            "properties": {
                                "step": {"type": "string", "description": "A short, concrete step."},
                                "status": {"type": "string", "enum": ["pending", "in_progress", "completed"]}
                            },
                            "required": ["step", "status"],
                            "additionalProperties": false
                        }
                    }
                },
                "required": ["explanation", "plan"],
                "additionalProperties": false,
            })),
            strict: Some(true),
            ..Default::default()
        }
    }

    fn call<'a>(
        &'a self,
        ctx: ToolContext<'a>,
        args: Value,
    ) -> BoxFuture<'a, anyhow::Result<String>> {
        Box::pin(async move {
            let args: Args = serde_json::from_value(args).context("invalid plan")?;
            validate(&args.plan)?;
            let done = args
                .plan
                .iter()
                .filter(|step| step.status == StepStatus::Completed)
                .count();
            let total = args.plan.len();
            ctx.events
                .send(Event::PlanUpdated {
                    explanation: args.explanation,
                    steps: args.plan,
                })
                .await
                .context("event receiver dropped")?;
            Ok(format!("plan updated: {done} of {total} steps completed"))
        })
    }
}

fn validate(plan: &[PlanStep]) -> anyhow::Result<()> {
    if plan.is_empty() || plan.len() > MAX_STEPS {
        bail!("a plan has 1 to {MAX_STEPS} steps");
    }
    if plan.iter().any(|step| step.step.trim().is_empty()) {
        bail!("every step needs a description");
    }
    let active = plan
        .iter()
        .filter(|step| step.status == StepStatus::InProgress)
        .count();
    if active > 1 {
        bail!("only one step can be in_progress at a time");
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    fn step(status: StepStatus) -> PlanStep {
        PlanStep {
            step: "do it".into(),
            status,
        }
    }

    #[test]
    fn allows_one_step_in_progress() {
        assert!(
            validate(&[
                step(StepStatus::Completed),
                step(StepStatus::InProgress),
                step(StepStatus::Pending)
            ])
            .is_ok()
        );
        assert!(validate(&[step(StepStatus::InProgress), step(StepStatus::InProgress)]).is_err());
        assert!(validate(&[]).is_err());
        let blank = PlanStep {
            step: " ".into(),
            status: StepStatus::Pending,
        };
        assert!(validate(&[blank]).is_err());
    }
}
