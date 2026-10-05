use std::{
    collections::HashMap,
    sync::{Arc, Mutex},
    time::Duration,
};

use anyhow::Context;
use futures::future::BoxFuture;
use serde_json::{Value, json};
use tokio::{
    sync::{mpsc, watch},
    time::{interval, timeout},
};
use tokio_util::sync::CancellationToken;
use tracing::Instrument;
use trex_harness::{
    agent::{Agent, Inbox, Journal, RunOutcome, Steering},
    attachment,
    event::Event,
    files, history,
    sandbox::{LazySandbox, Provided, SandboxProvider},
};
use trex_sandbox::{Sandbox, SandboxHealth, workspace_name};
use trex_store::{
    Store,
    sessions::{Finish, Session, SessionStatus, UsageRecord},
};
use uuid::Uuid;

use crate::api::{
    AppState,
    error::ApiError,
    events::{SessionEvent, to_api},
};
use crate::credits::{self, WorkspaceBudget};

const MAX_TURNS: usize = 50;
const SEND_ATTEMPTS: usize = 3;
// a run is resumed elsewhere once its lease misses a few heartbeats
const HEARTBEAT_INTERVAL: Duration = Duration::from_secs(10);
const RUN_STALE_AFTER: Duration = Duration::from_secs(30);
const RESUME_CHECK_INTERVAL: Duration = Duration::from_secs(10);
const RESUME_BATCH: i64 = 20;
const INSUFFICIENT_CREDITS: &str = "insufficient_credits";
const TITLE_TIMEOUT: Duration = Duration::from_secs(30);
const TITLE_ATTEMPTS: u32 = 2;
const FALLBACK_TITLE_CHARS: usize = 60;
const INSTRUCTIONS: &str = include_str!("instructions.md");

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
    workspace: Uuid,
    session: Uuid,
}

struct SessionJournal<'a> {
    store: &'a Store,
    workspace: Uuid,
    session: Uuid,
}

impl Journal for SessionJournal<'_> {
    fn save(&self, first: usize, items: Vec<Value>) -> BoxFuture<'_, anyhow::Result<()>> {
        Box::pin(async move {
            self.store
                .put_session_items(self.workspace, self.session, first, &items)
                .await
        })
    }
}

impl Inbox for SessionInbox<'_> {
    fn take(&self) -> BoxFuture<'_, anyhow::Result<Vec<Value>>> {
        Box::pin(async move {
            let queued = self
                .store
                .take_queued_messages(self.workspace, self.session)
                .await?;
            Ok(queued.into_iter().map(queued_item).collect())
        })
    }
}

// queues written before messages could carry attachments hold plain text
fn queued_item(value: Value) -> Value {
    match value {
        Value::String(text) => history::to_json(&[history::user_message(&text)])
            .ok()
            .and_then(|mut items| items.pop())
            .unwrap_or(Value::Null),
        item => item,
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
    OutOfCredits,
    LostLease,
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

    fn insert(&self, session: Uuid, id: RunId) -> (CancellationToken, watch::Receiver<()>) {
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
        (cancel, interrupts)
    }

    // the next run can start as soon as this one is finished in the store, so only its own handle goes
    fn remove(&self, session: Uuid, run: RunId) {
        let mut active = self.active.lock().expect("runs lock poisoned");
        if active.get(&session).is_some_and(|handle| handle.id == run) {
            active.remove(&session);
        }
    }
}

// admins can limit a workspace to some models; a chat on another model can't run
pub async fn require_model(state: &AppState, workspace: Uuid, model: &str) -> Result<(), ApiError> {
    let allowed = state.store.workspace_models(workspace).await?;
    if allowed.is_some_and(|ids| !ids.iter().any(|id| id == model)) {
        return Err(ApiError::Permission(format!(
            "this workspace can't use {model}; pick another model"
        )));
    }
    Ok(())
}

// claims the session, saves the new input, and runs the agent in the background
pub async fn start(
    state: &Arc<AppState>,
    workspace: Uuid,
    session: &Session,
    input: Vec<Value>,
) -> Result<(), ApiError> {
    require_model(state, workspace, &session.model).await?;
    credits::require(state, workspace).await?;
    if !claim(state, workspace, session, &input).await? {
        return Err(ApiError::Conflict(
            "a run is already in progress for this session".into(),
        ));
    }
    Ok(())
}

// starts a run with the message, or queues it for the running agent
pub async fn send_message(
    state: &Arc<AppState>,
    workspace: Uuid,
    session: &Session,
    message: Value,
    interrupt: bool,
) -> Result<Started, ApiError> {
    require_model(state, workspace, &session.model).await?;
    credits::require(state, workspace).await?;
    let input = [message];
    // the run can end between the two attempts, so they are retried a few times
    for _ in 0..SEND_ATTEMPTS {
        if claim(state, workspace, session, &input).await? {
            if session.title.is_none() {
                let text = history::message_text(&input[0]);
                tokio::spawn(name_session(
                    state.clone(),
                    workspace,
                    session.id,
                    session.model.clone(),
                    text,
                ));
            }
            return Ok(Started::Run);
        }
        if state
            .store
            .queue_message(workspace, session.id, &input[0])
            .await?
        {
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

// the model's title, retried once; when the model can't answer, the message's first line stands in
// so the chat never stays untitled
async fn name_session(
    state: Arc<AppState>,
    workspace: Uuid,
    session: Uuid,
    model: String,
    text: String,
) {
    let mut title = None;
    if let Some(model) = state.models.get(&model) {
        for attempt in 1..=TITLE_ATTEMPTS {
            match timeout(TITLE_TIMEOUT, model.title(&text)).await {
                Ok(Ok(generated)) => {
                    title = Some(generated);
                    break;
                }
                Ok(Err(error)) => {
                    tracing::warn!(session = %session, attempt, error = format!("{error:#}"), "failed to title session")
                }
                Err(_) => tracing::warn!(session = %session, attempt, "timed out titling session"),
            }
        }
    }
    let Some(title) = title.or_else(|| fallback_title(&text)) else {
        return;
    };
    match state
        .store
        .set_title_if_missing(workspace, session, &title)
        .await
    {
        Ok(true) => publish(&state, session, SessionEvent::SessionUpdated { title }).await,
        Ok(false) => {}
        Err(error) => {
            tracing::warn!(session = %session, error = format!("{error:#}"), "failed to save session title")
        }
    }
}

fn fallback_title(text: &str) -> Option<String> {
    let line = text.lines().map(str::trim).find(|line| !line.is_empty())?;
    let mut title: String = line.chars().take(FALLBACK_TITLE_CHARS).collect();
    if line.chars().count() > FALLBACK_TITLE_CHARS {
        title = format!("{}…", title.trim_end());
    }
    Some(title)
}

async fn claim(
    state: &Arc<AppState>,
    workspace: Uuid,
    session: &Session,
    input: &[Value],
) -> Result<bool, ApiError> {
    let run_id = Uuid::now_v7();
    if !state.store.start_run(workspace, session.id, run_id).await? {
        return Ok(false);
    }
    state
        .store
        .append_session_items(workspace, session.id, input)
        .await?;
    spawn(state, workspace, session.id, run_id, false);
    Ok(true)
}

// continues runs whose instance went away: after a restart, or when another instance crashed
pub async fn resume_stale_runs(state: Arc<AppState>) {
    let mut ticks = interval(RESUME_CHECK_INTERVAL);
    loop {
        ticks.tick().await;
        match state
            .store
            .claim_stale_runs(RUN_STALE_AFTER, RESUME_BATCH)
            .await
        {
            Ok(stale) => {
                for run in stale {
                    tracing::info!(session = %run.session, "resuming interrupted run");
                    spawn(&state, run.workspace, run.session, run.run, true);
                }
            }
            Err(error) => {
                tracing::warn!(
                    error = format!("{error:#}"),
                    "failed to look for interrupted runs"
                )
            }
        }
    }
}

fn spawn(state: &Arc<AppState>, workspace: Uuid, session: Uuid, run_id: RunId, resumed: bool) {
    let (cancel, interrupts) = state.runs.insert(session, run_id);
    let span = tracing::info_span!("run", session = %session, workspace = %workspace);
    let task = run(
        state.clone(),
        workspace,
        session,
        run_id,
        resumed,
        cancel,
        interrupts,
    );
    tokio::spawn(task.instrument(span));
}

#[allow(clippy::too_many_arguments)] // reason: the run's identity and its control channels, all distinct
async fn run(
    state: Arc<AppState>,
    workspace: Uuid,
    session: Uuid,
    run: RunId,
    resumed: bool,
    cancel: CancellationToken,
    interrupts: watch::Receiver<()>,
) {
    // a client that reloads mid-run keeps the items saved before this and replays the rest
    let items = state
        .store
        .count_session_items(workspace, session)
        .await
        .unwrap_or_else(|error| {
            tracing::warn!(session = %session, error = format!("{error:#}"), "failed to count session items");
            0
        });
    let started = if resumed {
        SessionEvent::RunResumed { items }
    } else {
        SessionEvent::RunStarted { items }
    };
    publish(&state, session, started).await;
    let result = tokio::select! {
        result = drive(&state, workspace, session, run, &cancel, interrupts) => result,
        _ = keep_lease(&state, session, run) => Ok(Finished::LostLease),
    };

    let event = match result {
        Ok(Finished::Completed) => Some(SessionEvent::RunCompleted),
        Ok(Finished::NeedsInput) => Some(SessionEvent::RunNeedsInput),
        Ok(Finished::Cancelled) => {
            stop(&state, workspace, session, run, SessionStatus::Idle, None).await;
            Some(SessionEvent::RunCancelled)
        }
        Ok(Finished::OutOfCredits) => {
            let message = "out of credits: the run stopped after its last step".to_owned();
            stop(
                &state,
                workspace,
                session,
                run,
                SessionStatus::Failed,
                Some(&message),
            )
            .await;
            Some(SessionEvent::RunFailed {
                error: message,
                code: Some(INSUFFICIENT_CREDITS),
            })
        }
        Ok(Finished::LostLease) => {
            tracing::warn!(session = %session, "another instance took over the run");
            None
        }
        Err(error) => {
            let message = format!("{error:#}");
            tracing::warn!(session = %session, error = message, "run failed");
            stop(
                &state,
                workspace,
                session,
                run,
                SessionStatus::Failed,
                Some(&message),
            )
            .await;
            Some(SessionEvent::RunFailed {
                error: message,
                code: None,
            })
        }
    };
    state.runs.remove(session, run);
    if let Some(event) = event {
        publish(&state, session, event).await;
    }
}

// returns once another instance owns the run; a failed renewal is retried, since the lease only
// goes stale after several missed heartbeats
async fn keep_lease(state: &AppState, session: Uuid, run: RunId) {
    let mut ticks = interval(HEARTBEAT_INTERVAL);
    loop {
        ticks.tick().await;
        match state.store.heartbeat_run(session, run).await {
            Ok(true) => {}
            Ok(false) => return,
            Err(error) => {
                tracing::warn!(session = %session, error = format!("{error:#}"), "failed to renew run lease")
            }
        }
    }
}

// messages queued for a run that won't read them are saved to history, so the next run sees them
async fn stop(
    state: &AppState,
    workspace: Uuid,
    session: Uuid,
    run: RunId,
    status: SessionStatus,
    error: Option<&str>,
) {
    let result: anyhow::Result<()> = async {
        loop {
            let queued = state.store.take_queued_messages(workspace, session).await?;
            let items: Vec<_> = queued.into_iter().map(queued_item).collect();
            state
                .store
                .append_session_items(workspace, session, &items)
                .await?;
            match state
                .store
                .finish_run(workspace, session, run, status, None, error)
                .await?
            {
                Finish::Finished | Finish::NotOwner => return Ok(()),
                Finish::MessagesQueued => {}
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
    workspace: Uuid,
    id: Uuid,
    run: RunId,
    cancel: &CancellationToken,
    interrupts: watch::Receiver<()>,
) -> anyhow::Result<Finished> {
    let session = state
        .store
        .session(workspace, id)
        .await?
        .context("session no longer exists")?;
    let model = state
        .models
        .get(&session.model)
        .with_context(|| format!("model {} is no longer configured", session.model))?;
    let project_instructions = match session.project_id {
        Some(project) => state
            .store
            .project(workspace, project)
            .await?
            .and_then(|project| project.instructions),
        None => None,
    };
    let reasoning_effort = session
        .reasoning_effort
        .as_deref()
        .map(|effort| model.check_effort(effort))
        .transpose()?;

    let provider = SessionSandbox {
        state,
        workspace,
        session: &session,
    };
    let sandbox = LazySandbox::new(&provider);

    let mut history = history::from_json(state.store.session_items(workspace, id).await?)?;
    let reasoning_from = state
        .store
        .start_reasoning(
            workspace,
            id,
            &session.model,
            i32::try_from(history.len()).context("session history is too long")?,
        )
        .await?;
    let inbox = SessionInbox {
        store: &state.store,
        workspace,
        session: id,
    };
    let budget = WorkspaceBudget {
        store: &state.store,
        workspace,
    };
    let journal = SessionJournal {
        store: &state.store,
        workspace,
        session: id,
    };
    let agent = Agent {
        workspace,
        library: &state.library,
        model,
        tools: &state.tools,
        openshell: &state.openshell,
        sandbox: &sandbox,
        instructions: Some(instructions(project_instructions.as_deref())),
        reasoning_effort,
        max_turns: MAX_TURNS,
        cache_key: Some(id.to_string()),
        steering: Some(Steering {
            inbox: &inbox,
            interrupts,
        }),
        journal: Some(&journal),
        fast: session.fast,
        budget: Some(&budget),
        reasoning_from,
    };

    // the agent saves every item as it goes, so whatever happens the session can be continued
    loop {
        let (tx, rx) = mpsc::channel(256);
        let forwarder = tokio::spawn(forward(state.clone(), workspace, id, session.fast, rx));
        let result = tokio::select! {
            result = agent.run(&mut history, &tx) => Some(result),
            _ = cancel.cancelled() => None,
        };
        drop(tx);
        let question = forwarder.await.context("event forwarder panicked")?;

        let (status, question, finished) = match result {
            None => return Ok(Finished::Cancelled),
            Some(Ok(RunOutcome::Completed)) => (SessionStatus::Idle, None, Finished::Completed),
            Some(Ok(RunOutcome::NeedsInput)) => (
                SessionStatus::NeedsInput,
                Some(question.context("run needs input but asked no question")?),
                Finished::NeedsInput,
            ),
            Some(Ok(RunOutcome::OutOfCredits)) => return Ok(Finished::OutOfCredits),
            Some(Err(error)) => return Err(error),
        };
        match state
            .store
            .finish_run(workspace, id, run, status, question.as_ref(), None)
            .await?
        {
            Finish::Finished => return Ok(finished),
            Finish::NotOwner => return Ok(Finished::LostLease),
            Finish::MessagesQueued => {
                tracing::info!(session = %id, "messages arrived as the run ended, continuing")
            }
        }
    }
}

// the date changes once a day, so it costs the prompt cache little
fn instructions(project: Option<&str>) -> String {
    let date = chrono::Utc::now().format("%Y-%m-%d");
    match project {
        Some(project) => format!(
            "{INSTRUCTIONS}\nToday's date is {date}.\n\n# Project instructions\n\nThe user set these for every chat in this project:\n\n{project}"
        ),
        None => format!("{INSTRUCTIONS}\nToday's date is {date}."),
    }
}

struct SessionSandbox<'a> {
    state: &'a AppState,
    workspace: Uuid,
    session: &'a Session,
}

impl SandboxProvider for SessionSandbox<'_> {
    fn provide<'a>(
        &'a self,
        events: &'a mpsc::Sender<Event>,
    ) -> BoxFuture<'a, anyhow::Result<Provided>> {
        Box::pin(async move {
            // the model only sees this as a tool error, so the operator needs it in the log
            let result = ensure_sandbox(self.state, self.workspace, self.session, events).await;
            match &result {
                Ok(provided) => {
                    sync_uploads(
                        self.state,
                        self.workspace,
                        self.session.id,
                        &provided.sandbox,
                    )
                    .await
                }
                Err(error) => {
                    tracing::warn!(session = %self.session.id, error = format!("{error:#}"), "failed to provide sandbox")
                }
            }
            result
        })
    }
}

// the session's sandbox for the files api: started if it was stopped, created only when `create`
// is set (writing a file), and none when there's nothing to show
// copies the conversation's attachments into the sandbox; existing files are left alone, so the
// agent's changes to an upload stay. best effort: a failure only means the tools can't see them
async fn sync_uploads(state: &AppState, workspace: Uuid, session: Uuid, sandbox: &Sandbox) {
    let result = async {
        let items = state.store.session_items(workspace, session).await?;
        let uploads = attachment::uploads(&items);
        if uploads.is_empty() {
            return anyhow::Ok(());
        }
        let present: Vec<String> = files::list(&state.openshell, sandbox)
            .await?
            .files
            .into_iter()
            .map(|file| file.path)
            .collect();
        for upload in uploads
            .iter()
            .filter(|upload| !present.contains(&upload.path))
        {
            let bytes = state
                .library
                .get_attachment(workspace, &upload.hash)
                .await?;
            files::write(&state.openshell, sandbox, &upload.path, bytes)
                .await
                .map_err(|error| anyhow::anyhow!("failed to copy {}: {error}", upload.path))?;
        }
        Ok(())
    }
    .await;
    if let Err(error) = result {
        tracing::warn!(%session, error = format!("{error:#}"), "failed to copy attachments into the sandbox");
    }
}

pub async fn files_sandbox(
    state: &AppState,
    workspace: Uuid,
    session: &Session,
    create: bool,
) -> anyhow::Result<Option<Sandbox>> {
    state.store.touch_session(workspace, session.id).await?;
    if session.sandbox.is_none() && !create {
        return Ok(None);
    }
    // nobody watches these events; a sandbox replaced here starts empty without the model being told
    let (events, _ignored) = mpsc::channel(16);
    if let Some(name) = &session.sandbox {
        let sandbox = Sandbox {
            workspace: workspace_name(workspace),
            name: name.clone(),
        };
        if state.openshell.health(&sandbox).await? != SandboxHealth::Usable && !create {
            return Ok(None);
        }
    }
    let sandbox = ensure_sandbox(state, workspace, session, &events)
        .await?
        .sandbox;
    sync_uploads(state, workspace, session.id, &sandbox).await;
    Ok(Some(sandbox))
}

// the sandbox is checked even when it isn't marked stopped, so one stopped by anything else recovers
async fn ensure_sandbox(
    state: &AppState,
    workspace: Uuid,
    session: &Session,
    events: &mpsc::Sender<Event>,
) -> anyhow::Result<Provided> {
    let Some(name) = &session.sandbox else {
        let sandbox = create_sandbox(state, workspace, session, events).await?;
        return Ok(Provided {
            sandbox,
            replaced: false,
        });
    };
    let sandbox = Sandbox {
        workspace: workspace_name(workspace),
        name: name.clone(),
    };
    let reason = match state.openshell.health(&sandbox).await? {
        SandboxHealth::Usable => {
            if session.sandbox_stopped {
                send(events, Event::SandboxStarting).await?;
            }
            state.openshell.start(&sandbox).await?;
            if session.sandbox_stopped {
                state
                    .store
                    .mark_sandbox_started(workspace, session.id)
                    .await?;
                send(events, Event::SandboxReady).await?;
            }
            return Ok(Provided {
                sandbox,
                replaced: false,
            });
        }
        SandboxHealth::Broken => {
            if let Err(error) = state.openshell.delete(&sandbox).await {
                tracing::warn!(session = %session.id, error = format!("{error:#}"), "failed to delete the broken sandbox");
            }
            "The sandbox stopped working"
        }
        SandboxHealth::Missing => "The sandbox no longer exists",
    };
    // a new sandbox lets the chat keep working; the user and the model are both told what was lost
    tracing::warn!(session = %session.id, sandbox = sandbox.name, reason, "replacing the session's sandbox");
    send(
        events,
        Event::SandboxReplaced {
            reason: reason.to_owned(),
        },
    )
    .await?;
    let sandbox = create_sandbox(state, workspace, session, events).await?;
    Ok(Provided {
        sandbox,
        replaced: true,
    })
}

async fn create_sandbox(
    state: &AppState,
    workspace: Uuid,
    session: &Session,
    events: &mpsc::Sender<Event>,
) -> anyhow::Result<Sandbox> {
    send(events, Event::SandboxCreating).await?;
    let openshell_workspace = state.openshell.ensure_workspace(workspace).await?;
    let sandbox = state
        .openshell
        .create(
            &openshell_workspace,
            Some(state.sandbox_image.clone()),
            Some(&state.sandbox_policy),
        )
        .await?;
    state
        .store
        .set_session_sandbox(workspace, session.id, &sandbox.name)
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
    workspace: Uuid,
    session: Uuid,
    fast: bool,
    mut rx: mpsc::Receiver<Event>,
) -> Option<Value> {
    let mut question = None;
    while let Some(event) = rx.recv().await {
        let mut charged = 0;
        match &event {
            Event::Usage(usage) => {
                let record = UsageRecord {
                    workspace_id: workspace,
                    session_id: session,
                    model: &usage.model,
                    input_tokens: usage.input_tokens,
                    cached_input_tokens: usage.cached_input_tokens,
                    cache_write_tokens: usage.cache_write_tokens,
                    output_tokens: usage.output_tokens,
                    reasoning_tokens: usage.reasoning_tokens,
                    duration_ms: usage.duration.as_millis() as u64,
                    first_token_ms: usage
                        .time_to_first_token
                        .map(|elapsed| elapsed.as_millis() as u64),
                };
                match credits::charge(&state, &record, fast).await {
                    Ok(credits) => charged = credits,
                    Err(error) => {
                        tracing::error!(session = %session, error = format!("{error:#}"), "failed to charge usage")
                    }
                }
            }
            Event::Question { call_id, questions } => {
                question = Some(json!({"call_id": call_id, "questions": questions}));
            }
            _ => {}
        }
        if let Some(mut event) = to_api(event) {
            if let SessionEvent::Usage { credits, .. } = &mut event {
                *credits = charged;
            }
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn falls_back_to_the_first_line() {
        assert_eq!(
            fallback_title("\n  Fix the login bug\nmore detail").as_deref(),
            Some("Fix the login bug")
        );
        let long = "a".repeat(80);
        assert_eq!(fallback_title(&long).map(|t| t.chars().count()), Some(61));
        assert_eq!(fallback_title("   \n "), None);
    }
}
