use std::{collections::HashSet, time::Duration};

use anyhow::Context;
use tokio::{sync::mpsc, time::sleep};
use trex_sandbox::{AccessRequest, AccessStatus, OpenShell};

use crate::{event::Event, sandbox::LazySandbox};

// a denied connection reaches the gateway's review queue a moment after the command fails
const SETTLE_POLLS: usize = 6;
const SETTLE_INTERVAL: Duration = Duration::from_millis(500);
const DECISION_INTERVAL: Duration = Duration::from_secs(1);
// how long a run waits for the user before it carries on without an answer
const DECISION_TIMEOUT: Duration = Duration::from_secs(10 * 60);

// network access a sandbox command was denied, held until the user approves or rejects it, so the
// model learns the answer in that command's result instead of the user having to tell it
pub struct AccessGate<'a> {
    pub openshell: &'a OpenShell,
    pub sandbox: &'a LazySandbox<'a>,
    // runs nobody is watching can't ask, so they report the block and carry on
    pub unattended: bool,
}

impl AccessGate<'_> {
    // the requests already pending before a command runs, which it isn't waiting on
    pub async fn pending(&self) -> HashSet<String> {
        let Some(sandbox) = self.sandbox.get_if_ready() else {
            return HashSet::new();
        };
        match self.openshell.pending_access(sandbox).await {
            Ok(requests) => requests.into_iter().map(|request| request.id).collect(),
            Err(error) => {
                // without the list the command still runs; it just can't wait for approval
                tracing::warn!(
                    error = format!("{error:#}"),
                    "failed to list access requests"
                );
                HashSet::new()
            }
        }
    }

    // after a command: the note for its output about access it was denied, once the user decided
    pub async fn settle(
        &self,
        before: &HashSet<String>,
        failed: bool,
        events: &mpsc::Sender<Event>,
    ) -> anyhow::Result<Option<String>> {
        let Some(sandbox) = self.sandbox.get_if_ready() else {
            return Ok(None);
        };
        let mut new = Vec::new();
        for poll in 0..if failed { SETTLE_POLLS } else { 1 } {
            if poll > 0 {
                sleep(SETTLE_INTERVAL).await;
            }
            new = self
                .openshell
                .pending_access(sandbox)
                .await?
                .into_iter()
                .filter(|request| !before.contains(&request.id))
                .collect::<Vec<_>>();
            if !new.is_empty() {
                break;
            }
        }
        if new.is_empty() {
            return Ok(None);
        }
        if self.unattended {
            return Ok(Some(format!(
                "[network access to {} was blocked; it needs the user's approval, and no one can approve it during this run]",
                hosts(&new)
            )));
        }
        for request in &new {
            events
                .send(Event::AccessRequest(request.clone()))
                .await
                .context("event receiver dropped")?;
        }
        let decided = self.wait_for_decisions(sandbox, &new).await?;
        Ok(Some(note(&new, &decided)))
    }

    async fn wait_for_decisions(
        &self,
        sandbox: &trex_sandbox::Sandbox,
        requests: &[AccessRequest],
    ) -> anyhow::Result<Vec<AccessStatus>> {
        let started = tokio::time::Instant::now();
        loop {
            let statuses = self.openshell.access_statuses(sandbox).await?;
            let decided: Vec<_> = requests
                .iter()
                .map(|request| {
                    statuses
                        .get(&request.id)
                        .copied()
                        .unwrap_or(AccessStatus::Rejected)
                })
                .collect();
            if !decided.contains(&AccessStatus::Pending) || started.elapsed() >= DECISION_TIMEOUT {
                return Ok(decided);
            }
            sleep(DECISION_INTERVAL).await;
        }
    }
}

fn hosts(requests: &[AccessRequest]) -> String {
    let hosts: Vec<_> = requests
        .iter()
        .flat_map(|request| request.endpoints.iter().map(String::as_str))
        .collect();
    if hosts.is_empty() {
        "a host".to_owned()
    } else {
        hosts.join(", ")
    }
}

// what the model reads about each request, grouped by the user's answer
fn note(requests: &[AccessRequest], decided: &[AccessStatus]) -> String {
    let with = |status: AccessStatus| -> Vec<AccessRequest> {
        requests
            .iter()
            .zip(decided)
            .filter(|(_, decision)| **decision == status)
            .map(|(request, _)| request.clone())
            .collect()
    };
    let mut lines = Vec::new();
    let approved = with(AccessStatus::Approved);
    if !approved.is_empty() {
        lines.push(format!(
            "[the user approved network access to {}; run the command again]",
            hosts(&approved)
        ));
    }
    let rejected = with(AccessStatus::Rejected);
    if !rejected.is_empty() {
        lines.push(format!("[the user rejected network access to {}; don't retry it, find another way or tell them what you couldn't do]", hosts(&rejected)));
    }
    let pending = with(AccessStatus::Pending);
    if !pending.is_empty() {
        lines.push(format!("[network access to {} is still waiting for the user's approval; carry on without it or ask them]", hosts(&pending)));
    }
    lines.join("\n")
}

#[cfg(test)]
mod tests {
    use super::*;

    fn request(id: &str, host: &str) -> AccessRequest {
        AccessRequest {
            id: id.into(),
            review_token: String::new(),
            rule_name: String::new(),
            endpoints: vec![host.into()],
            binary: "/usr/bin/curl".into(),
            rationale: String::new(),
            security_notes: String::new(),
            hit_count: 1,
        }
    }

    #[test]
    fn tells_the_model_each_answer() {
        let requests = [
            request("a", "github.com:443"),
            request("b", "evil.test:443"),
        ];
        assert_eq!(
            note(&requests, &[AccessStatus::Approved, AccessStatus::Rejected]),
            "[the user approved network access to github.com:443; run the command again]\n\
             [the user rejected network access to evil.test:443; don't retry it, find another way or tell them what you couldn't do]"
        );
        assert_eq!(
            note(&requests[..1], &[AccessStatus::Pending]),
            "[network access to github.com:443 is still waiting for the user's approval; carry on without it or ask them]"
        );
    }
}
