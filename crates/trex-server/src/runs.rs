use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
};

use anyhow::Context;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::sync::{mpsc, watch};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use trex_harness::{
    agent::{Agent, Inbox, RunOutcome, Steering},
    event::Event,
    history,
    model::parse_effort,
    sandbox::{LazySandbox, SandboxProvider},
};
use trex_sandbox::{Sandbox, workspace_name};
use trex_store::{
    Store,
    sessions::{Session, SessionStatus, UsageRecord},
};
use uuid::Uuid;

use crate::api::{
    AppState,
    error::ApiError,
    events::{SessionEvent, to_api},
};

const MAX_TURNS: usize = 50;
const SEND_ATTEMPTS: usize = 3;
const INSTRUCTIONS: &str = "You are trex, an autonomous agent working for the user inside a Linux sandbox. \
Your working directory is /sandbox, which persists for this conversation. Use your tools to do the work rather than \
describing it: run commands, read and edit files, search the web when needed, and verify your results. \
Python, Node, Go, Rust and common build tools are installed; install anything else with pip, uv, npm, go, cargo, \
or micromamba (apt is not available). The user's personal library is reachable with the library tools; save \
deliverables there when the user should keep them. Ask the user only when you need a decision or information you \
cannot find yourself. Be concise in your replies.";

#[derive(Default)]
pub struct Runs {
    active: Mutex<HashMap<Uuid, RunHandle>>,
}

struct RunHandle {
    id: RunId,
    cancel: CancellationToken,
    interrupt: watch::Sender<()>,
}

struct SessionInbox<'a> {
    store: &'a Store,
    user: Uuid,
    session: Uuid,
}

impl Inbox for SessionInbox<'_> {
    fn take(&self) -> BoxFuture<'_, anyhow::Result<Vec<String>>> {
        Box::pin(self.store.take_queued_messages(self.user, self.session))
    }
}

/// How `start` handled the input.
pub enum Started {
    Run,
    // the session was already running; the agent reads the message between turns
    Queued,
}

enum Finished {
    Completed,
    NeedsInput,
    Cancelled,
}

type RunId = Uuid;

impl Runs {
    pub fn cancel(&self, session: Uuid) -> bool {
        let active = self.active.lock().expect("runs lock poisoned");
        match active.get(&session) {
            Some(run) => {
                run.cancel.cancel();
                true
            }
            None => false,
        }
    }

    // a run on another trex instance isn't reachable, so it reads the message at its next turn
    fn interrupt(&self, session: Uuid) {
        let active = self.active.lock().expect("runs lock poisoned");
        if let Some(run) = active.get(&session) {
            run.interrupt.send_replace(());
        }
    }

    fn insert(&self, session: Uuid) -> (RunId, CancellationToken, watch::Receiver<()>) {
        let id = Uuid::now_v7();
        let cancel = CancellationToken::new();
        let (interrupt, interrupts) = watch::channel(());
        self.active.lock().expect("runs lock poisoned").insert(
            session,
            RunHandle {
                id,
                cancel: cancel.clone(),
                interrupt,
            },
        );
        (id, cancel, interrupts)
    }

    // the next run can start as soon as this one is finished in the store, so only its own handle goes
    fn remove(&self, session: Uuid, run: RunId) {
        let mut active = self.active.lock().expect("runs lock poisoned");
        if active.get(&session).is_some_and(|handle| handle.id == run) {
            active.remove(&session);
        }
    }
}

// claims the session, saves the new input, and runs the agent in the background
pub async fn start(
    state: &Arc<AppState>,
    user: Uuid,
    session: &Session,
    input: Vec<Value>,
) -> Result<(), ApiError> {
    if !claim(state, user, session, &input).await? {
        return Err(ApiError::Conflict(
            "a run is already in progress for this session".into(),
        ));
    }
    Ok(())
}

// starts a run with the message, or queues it for the running agent
pub async fn send_message(
    state: &Arc<AppState>,
    user: Uuid,
    session: &Session,
    content: &str,
    interrupt: bool,
) -> Result<Started, ApiError> {
    let input = history::to_json(&[history::user_message(content)])?;
    // the run can end between the two attempts, so they are retried a few times
    for _ in 0..SEND_ATTEMPTS {
        if claim(state, user, session, &input).await? {
            return Ok(Started::Run);
        }
        if state.store.queue_message(user, session.id, content).await? {
            if interrupt {
                state.runs.interrupt(session.id);
            }
            return Ok(Started::Queued);
        }
    }
    Err(ApiError::Conflict(
        "the session's run is changing state, try again".into(),
    ))
}

async fn claim(
    state: &Arc<AppState>,
    user: Uuid,
    session: &Session,
    input: &[Value],
) -> Result<bool, ApiError> {
    if state.store.start_run(user, session.id).await?.is_none() {
        return Ok(false);
    }
    state
        .store
        .append_session_items(user, session.id, input)
        .await?;

    let (run_id, cancel, interrupts) = state.runs.insert(session.id);
    let span = tracing::info_span!("run", session = %session.id, user = %user);
    tokio::spawn(run(state.clone(), user, session.id, run_id, cancel, interrupts).instrument(span));
    Ok(true)
}

async fn run(
    state: Arc<AppState>,
    user: Uuid,
    session: Uuid,
    run: RunId,
    cancel: CancellationToken,
    interrupts: watch::Receiver<()>,
) {
    publish(&state, session, SessionEvent::RunStarted).await;
    let result = drive(&state, user, session, &cancel, interrupts).await;

    let event = match result {
        Ok(Finished::Completed) => SessionEvent::RunCompleted,
        Ok(Finished::NeedsInput) => SessionEvent::RunNeedsInput,
        Ok(Finished::Cancelled) => {
            stop(&state, user, session, SessionStatus::Idle, None).await;
            SessionEvent::RunCancelled
        }
        Err(error) => {
            let message = format!("{error:#}");
            tracing::warn!(session = %session, error = message, "run failed");
            stop(&state, user, session, SessionStatus::Failed, Some(&message)).await;
            SessionEvent::RunFailed { error: message }
        }
    };
    state.runs.remove(session, run);
    publish(&state, session, event).await;
}

// messages queued for a run that won't read them are saved to history, so the next run sees them
async fn stop(
    state: &AppState,
    user: Uuid,
    session: Uuid,
    status: SessionStatus,
    error: Option<&str>,
) {
    let result: anyhow::Result<()> = async {
        loop {
            let queued = state.store.take_queued_messages(user, session).await?;
            let items: Vec<_> = queued.iter().map(|m| history::user_message(m)).collect();
            state
                .store
                .append_session_items(user, session, &history::to_json(&items)?)
                .await?;
            if state
                .store
                .finish_run(user, session, status, None, error)
                .await?
            {
                return Ok(());
            }
        }
    }
    .await;
    if let Err(error) = result {
        tracing::error!(session = %session, error = format!("{error:#}"), "failed to finish run");
    }
}

// completed and needs-input runs are finished here, because a message that arrives as the run
// ends must continue it instead
async fn drive(
    state: &Arc<AppState>,
    user: Uuid,
    id: Uuid,
    cancel: &CancellationToken,
    interrupts: watch::Receiver<()>,
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

    let provider = SessionSandbox {
        state,
        user,
        session: &session,
    };
    let sandbox = LazySandbox::new(&provider);

    let mut history = history::from_json(state.store.session_items(user, id).await?)?;
    let inbox = SessionInbox {
        store: &state.store,
        user,
        session: id,
    };
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
        steering: Some(Steering {
            inbox: &inbox,
            interrupts,
        }),
    };

    loop {
        let saved = history.len();
        let (tx, rx) = mpsc::channel(256);
        let forwarder = tokio::spawn(forward(state.clone(), user, id, rx));
        let result = tokio::select! {
            result = agent.run(&mut history, &tx) => Some(result),
            _ = cancel.cancelled() => None,
        };
        drop(tx);
        let question = forwarder.await.context("event forwarder panicked")?;

        // history is saved whatever happened, so a failed or cancelled run can be continued
        let produced = history::to_json(&history[saved..])?;
        state
            .store
            .append_session_items(user, id, &produced)
            .await?;

        let (status, question, finished) = match result {
            None => return Ok(Finished::Cancelled),
            Some(Ok(RunOutcome::Completed)) => (SessionStatus::Idle, None, Finished::Completed),
            Some(Ok(RunOutcome::NeedsInput)) => (
                SessionStatus::NeedsInput,
                Some(question.context("run needs input but asked no question")?),
                Finished::NeedsInput,
            ),
            Some(Err(error)) => return Err(error),
        };
        if state
            .store
            .finish_run(user, id, status, question.as_ref(), None)
            .await?
        {
            return Ok(finished);
        }
        tracing::info!(session = %id, "messages arrived as the run ended, continuing");
    }
}

struct SessionSandbox<'a> {
    state: &'a AppState,
    user: Uuid,
    session: &'a Session,
}

impl SandboxProvider for SessionSandbox<'_> {
    fn provide<'a>(
        &'a self,
        events: &'a mpsc::Sender<Event>,
    ) -> BoxFuture<'a, anyhow::Result<Sandbox>> {
        Box::pin(ensure_sandbox(self.state, self.user, self.session, events))
    }
}

// the sandbox is checked even when it isn't marked stopped, so one stopped by anything else recovers
async fn ensure_sandbox(
    state: &AppState,
    user: Uuid,
    session: &Session,
    events: &mpsc::Sender<Event>,
) -> anyhow::Result<Sandbox> {
    if let Some(name) = &session.sandbox {
        let sandbox = Sandbox {
            workspace: workspace_name(user),
            name: name.clone(),
        };
        if session.sandbox_stopped {
            send(events, Event::SandboxStarting).await?;
        }
        state.openshell.start(&sandbox).await?;
        if session.sandbox_stopped {
            state.store.mark_sandbox_started(user, session.id).await?;
            send(events, Event::SandboxReady).await?;
        }
        return Ok(sandbox);
    }

    send(events, Event::SandboxCreating).await?;
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
    send(events, Event::SandboxReady).await?;
    Ok(sandbox)
}

async fn send(events: &mpsc::Sender<Event>, event: Event) -> anyhow::Result<()> {
    events.send(event).await.context("event receiver dropped")
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
