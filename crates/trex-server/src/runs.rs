use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use serde_json::{Value, json};
use tokio::sync::mpsc;
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use trex_harness::{
    agent::{Agent, RunOutcome},
    event::Event,
    history,
    model::parse_effort,
};
use trex_sandbox::{Sandbox, workspace_name};
use trex_store::sessions::{Session, SessionStatus, UsageRecord};
use uuid::Uuid;

use crate::api::{
    AppState,
    error::ApiError,
    events::{SessionEvent, to_api},
};

const MAX_TURNS: usize = 50;
const INSTRUCTIONS: &str = "You are trex, an autonomous agent working for the user inside a Linux sandbox. \
Your working directory is /sandbox, which persists for this conversation. Use your tools to do the work rather than \
describing it: run commands, read and edit files, search the web when needed, and verify your results. \
Python, Node, Go, Rust and common build tools are installed; install anything else with pip, uv, npm, go, cargo, \
or micromamba (apt is not available). The user's personal library is reachable with the library tools; save \
deliverables there when the user should keep them. Ask the user only when you need a decision or information you \
cannot find yourself. Be concise in your replies.";

#[derive(Default)]
pub struct Runs {
    active: Mutex<HashMap<Uuid, CancellationToken>>,
}

enum Finished {
    Completed,
    NeedsInput(Value),
    Cancelled,
}

impl Runs {
    pub fn cancel(&self, session: Uuid) -> bool {
        let active = self.active.lock().expect("runs lock poisoned");
        match active.get(&session) {
            Some(token) => {
                token.cancel();
                true
            }
            None => false,
        }
    }

    fn insert(&self, session: Uuid) -> CancellationToken {
        let token = CancellationToken::new();
        self.active
            .lock()
            .expect("runs lock poisoned")
            .insert(session, token.clone());
        token
    }

    fn remove(&self, session: Uuid) {
        self.active
            .lock()
            .expect("runs lock poisoned")
            .remove(&session);
    }
}

// claims the session, saves the new input, and runs the agent in the background
pub async fn start(
    state: &Arc<AppState>,
    user: Uuid,
    session: &Session,
    input: Vec<Value>,
) -> Result<(), ApiError> {
    if state.store.start_run(user, session.id).await?.is_none() {
        return Err(ApiError::Conflict(
            "a run is already in progress for this session".into(),
        ));
    }
    state
        .store
        .append_session_items(user, session.id, &input)
        .await?;

    let token = state.runs.insert(session.id);
    let span = tracing::info_span!("run", session = %session.id, user = %user);
    tokio::spawn(run(state.clone(), user, session.id, token).instrument(span));
    Ok(())
}

async fn run(state: Arc<AppState>, user: Uuid, session: Uuid, token: CancellationToken) {
    publish(&state, session, SessionEvent::RunStarted).await;
    let result = drive(&state, user, session, &token).await;
    state.runs.remove(session);

    let (status, question, error, event) = match result {
        Ok(Finished::Completed) => (SessionStatus::Idle, None, None, SessionEvent::RunCompleted),
        Ok(Finished::Cancelled) => (SessionStatus::Idle, None, None, SessionEvent::RunCancelled),
        Ok(Finished::NeedsInput(question)) => (
            SessionStatus::NeedsInput,
            Some(question),
            None,
            SessionEvent::RunNeedsInput,
        ),
        Err(error) => {
            let message = format!("{error:#}");
            tracing::warn!(session = %session, error = message, "run failed");
            let event = SessionEvent::RunFailed {
                error: message.clone(),
            };
            (SessionStatus::Failed, None, Some(message), event)
        }
    };

    if let Err(error) = state
        .store
        .finish_run(user, session, status, question.as_ref(), error.as_deref())
        .await
    {
        tracing::error!(session = %session, error = format!("{error:#}"), "failed to finish run");
    }
    publish(&state, session, event).await;
}

async fn drive(
    state: &Arc<AppState>,
    user: Uuid,
    id: Uuid,
    token: &CancellationToken,
) -> anyhow::Result<Finished> {
    let session = state
        .store
        .session(user, id)
        .await?
        .context("session no longer exists")?;
    let model = state
        .models
        .get(&session.model)
        .with_context(|| format!("model {} is no longer configured", session.model))?;
    let reasoning_effort = session
        .reasoning_effort
        .as_deref()
        .map(parse_effort)
        .transpose()?;

    let sandbox = tokio::select! {
        sandbox = ensure_sandbox(state, user, &session) => sandbox?,
        _ = token.cancelled() => return Ok(Finished::Cancelled),
    };

    let mut history = history::from_json(state.store.session_items(user, id).await?)?;
    let saved = history.len();

    let (tx, rx) = mpsc::channel(256);
    let forwarder = tokio::spawn(forward(state.clone(), user, id, rx));

    let agent = Agent {
        user,
        library: &state.library,
        model,
        tools: &state.tools,
        openshell: &state.openshell,
        sandbox: &sandbox,
        instructions: Some(INSTRUCTIONS.into()),
        reasoning_effort,
        max_turns: MAX_TURNS,
        cache_key: Some(id.to_string()),
    };
    let result = tokio::select! {
        result = agent.run(&mut history, &tx) => Some(result),
        _ = token.cancelled() => None,
    };
    drop(tx);
    let question = forwarder.await.context("event forwarder panicked")?;

    // history is saved whatever happened, so a failed or cancelled run can be continued
    let produced = history::to_json(&history[saved..])?;
    state
        .store
        .append_session_items(user, id, &produced)
        .await?;

    match result {
        None => Ok(Finished::Cancelled),
        Some(Ok(RunOutcome::Completed)) => Ok(Finished::Completed),
        Some(Ok(RunOutcome::NeedsInput)) => Ok(Finished::NeedsInput(
            question.context("run needs input but asked no question")?,
        )),
        Some(Err(error)) => Err(error),
    }
}

async fn ensure_sandbox(
    state: &AppState,
    user: Uuid,
    session: &Session,
) -> anyhow::Result<Sandbox> {
    if let Some(name) = &session.sandbox {
        return Ok(Sandbox {
            workspace: workspace_name(user),
            name: name.clone(),
        });
    }

    publish(state, session.id, SessionEvent::SandboxCreating).await;
    let workspace = state.openshell.ensure_workspace(user).await?;
    let sandbox = state
        .openshell
        .create(
            &workspace,
            Some(state.sandbox_image.clone()),
            Some(&state.sandbox_policy),
        )
        .await?;
    state
        .store
        .set_session_sandbox(user, session.id, &sandbox.name)
        .await?;
    publish(state, session.id, SessionEvent::SandboxReady).await;
    Ok(sandbox)
}

// returns the question the model asked, if any, so the session can wait for an answer
async fn forward(
    state: Arc<AppState>,
    user: Uuid,
    session: Uuid,
    mut rx: mpsc::Receiver<Event>,
) -> Option<Value> {
    let mut question = None;
    while let Some(event) = rx.recv().await {
        match &event {
            Event::Usage(usage) => {
                let record = UsageRecord {
                    user_id: user,
                    session_id: session,
                    model: &usage.model,
                    input_tokens: usage.input_tokens,
                    cached_input_tokens: usage.cached_input_tokens,
                    cache_write_tokens: usage.cache_write_tokens,
                    output_tokens: usage.output_tokens,
                    reasoning_tokens: usage.reasoning_tokens,
                };
                if let Err(error) = state.store.record_usage(&record).await {
                    tracing::error!(session = %session, error = format!("{error:#}"), "failed to record usage");
                }
            }
            Event::Question { call_id, questions } => {
                question = Some(json!({"call_id": call_id, "questions": questions}));
            }
            _ => {}
        }
        if let Some(event) = to_api(event) {
            publish(&state, session, event).await;
        }
    }
    question
}

// a lost event only degrades the live view; the run itself and its saved history are unaffected
async fn publish(state: &AppState, session: Uuid, event: SessionEvent) {
    let result = match serde_json::to_value(&event) {
        Ok(value) => state.store.publish_event(session, &value).await,
        Err(error) => Err(error.into()),
    };
    if let Err(error) = result {
        tracing::warn!(session = %session, error = format!("{error:#}"), "failed to publish event");
    }
}
