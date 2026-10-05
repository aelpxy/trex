use std::{
    collections::{HashMap, HashSet},
    time::{Duration, Instant},
};

use anyhow::{Context, anyhow, bail};
use async_openai::{
    error::OpenAIError,
    types::responses::{
        FunctionCallOutputItemParam, FunctionToolCall, InputItem, Item, OutputItem,
        ReasoningEffort, Response, ResponseErrorCode, ResponseStreamEvent, Tool,
    },
};
use futures::{StreamExt, future::BoxFuture, stream::FuturesUnordered};
use serde_json::Value;
use tokio::{
    sync::{mpsc, watch},
    time::{sleep, timeout},
};
use trex_sandbox::OpenShell;
use trex_store::library::Library;
use uuid::Uuid;

use crate::{
    access::AccessGate,
    attachment,
    event::{Event, Usage},
    history,
    model::{Model, Turn},
    question::{self, ASK_USER},
    sandbox::LazySandbox,
    tool::{ToolContext, ToolOutput, Tools},
};

// the model would otherwise assume files and installs from earlier in the conversation still exist
const SANDBOX_REPLACED_NOTE: &str = "[The sandbox stopped working and was replaced with a fresh one. Files, installed packages and processes from earlier in this conversation are gone; recreate what you need, or load it from the library.]";
const ACCESS_POLL_INTERVAL: Duration = Duration::from_secs(5);
const BASH: &str = "bash";
// attempts per model request; the client already retries failed connections and statuses beneath this
const MAX_ATTEMPTS: u32 = 5;
const RETRY_BASE_DELAY: Duration = Duration::from_secs(2);
const RETRY_MAX_DELAY: Duration = Duration::from_secs(60);
// long reasoning can go quiet for minutes, so only a much longer silence counts as a stall
const STREAM_IDLE_TIMEOUT: Duration = Duration::from_secs(300);
// leaves room for the summary request itself and the output of the next turn
const COMPACT_AT_PERCENT: u64 = 80;
const RECENT_USER_MESSAGE_CHARS: usize = 40_000;
// a transcript that is summarized as text is cut to about half the window, at ~4 chars per token
const TRANSCRIPT_CHARS_PER_WINDOW_TOKEN: u64 = 2;
const CHARS_PER_TOKEN: usize = 4;
const SUMMARY_PROMPT: &str = "Stop working on the task and write a handoff summary of this conversation so far, \
so that you can continue it later from the summary alone. Do not call any tools. Include:\n\
- the user's goal and every requirement, preference and constraint they stated\n\
- what has been done: files created or changed (with paths), commands run and what they showed, decisions made and why\n\
- the current state, including anything broken, failing or unverified\n\
- what remains to be done, in order, and the immediate next step\n\
Be specific and complete; anything left out is forgotten. Reply with only the summary.";

// messages the user sent while a run was working, delivered between turns
pub trait Inbox: Send + Sync {
    // removes and returns the waiting user messages as history items, oldest first
    fn take(&self) -> BoxFuture<'_, anyhow::Result<Vec<Value>>>;
}

// persists history as it grows; `first` is the position of `items[0]` in history, so saving the
// same items again changes nothing
pub trait Journal: Send + Sync {
    fn save(&self, first: usize, items: Vec<Value>) -> BoxFuture<'_, anyhow::Result<()>>;
}

pub struct Steering<'a> {
    pub inbox: &'a dyn Inbox,
    // each change aborts the current turn; the run then continues with the inbox
    pub interrupts: watch::Receiver<()>,
}

// whether the run may make another model request
pub trait Budget: Send + Sync {
    fn exhausted(&self) -> BoxFuture<'_, anyhow::Result<bool>>;
}

#[derive(Debug, PartialEq)]
pub enum RunOutcome {
    Completed,
    // the budget ran out; the step in progress was finished first
    OutOfCredits,
    // the model asked the user something; push question::answer_item and run again to continue
    NeedsInput,
}

pub struct Agent<'a> {
    pub workspace: Uuid,
    pub library: &'a Library,
    pub model: &'a Model,
    pub tools: &'a Tools,
    pub openshell: &'a OpenShell,
    pub sandbox: &'a LazySandbox<'a>,
    pub instructions: Option<String>,
    pub reasoning_effort: Option<ReasoningEffort>,
    pub max_turns: usize,
    // requests sharing a key are routed to the same prompt cache
    pub cache_key: Option<String>,
    pub steering: Option<Steering<'a>>,
    pub journal: Option<&'a dyn Journal>,
    pub fast: bool,
    pub budget: Option<&'a dyn Budget>,
    // reasoning items before this position came from another model, which can't read them
    pub reasoning_from: usize,
    // nobody is there to answer, as in a scheduled run, so the agent can't ask questions
    pub unattended: bool,
}

#[derive(Clone, Copy)]
struct Timing {
    duration: Duration,
    first_token: Option<Duration>,
}

struct Sampled {
    items: Vec<InputItem>,
    calls: Vec<FunctionToolCall>,
    text: String,
    context_tokens: u64,
}

enum Step {
    Continue,
    Completed,
    NeedsInput,
}

enum Failure {
    Retryable(anyhow::Error),
    ContextOverflow,
    Fatal(anyhow::Error),
}

impl From<anyhow::Error> for Failure {
    fn from(error: anyhow::Error) -> Self {
        Self::Fatal(error)
    }
}

// how a model request is made: whether tools may be called and whether output is shown live
#[derive(Clone, Copy)]
struct Mode {
    tools: bool,
    live: bool,
}

const WORK: Mode = Mode {
    tools: true,
    live: true,
};
const SUMMARIZE: Mode = Mode {
    tools: false,
    live: false,
};

impl Agent<'_> {
    // appends every item the run produces to history so the next run continues from it; with a
    // journal, `history` must already be saved when the run starts
    pub async fn run(
        &self,
        history: &mut Vec<InputItem>,
        events: &mpsc::Sender<Event>,
    ) -> anyhow::Result<RunOutcome> {
        let mut saved = history.len();
        let result = tokio::select! {
            result = self.run_turns(history, events, &mut saved) => result,
            error = self.watch_access(events) => Err(error),
        };
        let flushed = self.save(history, &mut saved).await;
        let outcome = result?;
        flushed?;
        Ok(outcome)
    }

    async fn run_turns(
        &self,
        history: &mut Vec<InputItem>,
        events: &mpsc::Sender<Event>,
        saved: &mut usize,
    ) -> anyhow::Result<RunOutcome> {
        history::close_dangling_calls(history);
        self.save(history, saved).await?;

        for turn in 0..self.max_turns {
            tracing::debug!(turn, model = self.model.id(), "starting turn");
            self.receive_messages(history, events, saved).await?;
            // a resumed run may have stopped right after the model's final reply
            if turn == 0 && history::ends_with_reply(history) {
                send(events, Event::Done).await?;
                return Ok(RunOutcome::Completed);
            }
            // the first request is allowed, since the run was only started with credits left
            if turn > 0
                && let Some(budget) = self.budget
                && budget.exhausted().await?
            {
                tracing::info!(turn, "stopping run, out of credits");
                return Ok(RunOutcome::OutOfCredits);
            }

            // an interrupt sent before the messages were taken is already answered by them
            let mut interrupts = self.steering.as_ref().map(|s| s.interrupts.clone());
            if let Some(interrupts) = &mut interrupts {
                interrupts.mark_unchanged();
            }
            let step = tokio::select! {
                step = self.step(history, events, saved) => Some(step?),
                _ = interrupted(interrupts) => None,
            };
            match step {
                Some(Step::Continue) => {}
                Some(Step::Completed) => {
                    if self.receive_messages(history, events, saved).await? {
                        continue;
                    }
                    // models sometimes stop with steps left; one reminder per plan gets them to finish
                    if let Some(steps) = history::unfinished_plan(history) {
                        tracing::info!(
                            turn,
                            steps = steps.len(),
                            "reminding the model of its plan"
                        );
                        history.push(history::plan_reminder(&steps));
                        self.save(history, saved).await?;
                        continue;
                    }
                    send(events, Event::Done).await?;
                    return Ok(RunOutcome::Completed);
                }
                Some(Step::NeedsInput) => return Ok(RunOutcome::NeedsInput),
                None => {
                    tracing::info!(turn, "turn interrupted by the user");
                    history::close_dangling_calls(history);
                    self.save(history, saved).await?;
                    send(events, Event::Interrupted).await?;
                }
            }
        }

        bail!(
            "run stopped after reaching the limit of {} turns",
            self.max_turns
        )
    }

    // writing an item twice is harmless, so a save cut short by an interrupt is simply repeated
    async fn save(&self, history: &[InputItem], saved: &mut usize) -> anyhow::Result<()> {
        if let Some(journal) = self.journal
            && *saved < history.len()
        {
            journal
                .save(*saved, history::to_json(&history[*saved..])?)
                .await?;
        }
        *saved = history.len();
        Ok(())
    }

    // appends messages the user sent while the agent was working; true if there were any
    async fn receive_messages(
        &self,
        history: &mut Vec<InputItem>,
        events: &mpsc::Sender<Event>,
        saved: &mut usize,
    ) -> anyhow::Result<bool> {
        let Some(steering) = &self.steering else {
            return Ok(false);
        };
        let messages = steering.inbox.take().await?;
        let received = !messages.is_empty();
        for message in messages {
            let content = history::message_text(&message);
            history.extend(history::from_json(vec![message])?);
            send(events, Event::MessageReceived { content }).await?;
        }
        self.save(history, saved).await?;
        Ok(received)
    }

    // one model response and the tools it called; dropping it midway leaves history valid once
    // dangling calls are closed, because each item is saved as soon as it exists
    async fn step(
        &self,
        history: &mut Vec<InputItem>,
        events: &mpsc::Sender<Event>,
        saved: &mut usize,
    ) -> anyhow::Result<Step> {
        let sampled = self.sample(history, events).await?;
        history.extend(sampled.items);
        self.save(history, saved).await?;
        if sampled.calls.is_empty() {
            return Ok(Step::Completed);
        }

        let (asks, calls): (Vec<_>, Vec<_>) = sampled
            .calls
            .into_iter()
            .partition(|call| call.name == ASK_USER);

        let mut running: FuturesUnordered<_> = calls
            .iter()
            .map(|call| async move { (call, self.call_tool(call, events).await) })
            .collect();
        let mut output_chars = 0;
        while let Some((call, output)) = running.next().await {
            let output = output?;
            output_chars += output.text().len();
            history.push(content_output_item(call.call_id.clone(), output));
            self.save(history, saved).await?;
        }

        let mut asked = false;
        for call in asks {
            match question::parse(&call.arguments) {
                Ok(questions) => {
                    send(
                        events,
                        Event::Question {
                            call_id: call.call_id,
                            questions,
                        },
                    )
                    .await?;
                    asked = true;
                }
                Err(error) => {
                    history.push(output_item(call.call_id, format!("error: {error:#}")));
                }
            }
        }
        self.save(history, saved).await?;
        if asked {
            return Ok(Step::NeedsInput);
        }

        let context_tokens = sampled.context_tokens + (output_chars / CHARS_PER_TOKEN) as u64;
        if context_tokens >= self.compact_threshold() {
            tracing::info!(
                context_tokens,
                model = self.model.id(),
                "compacting context"
            );
            self.compact(history, events).await?;
            self.save(history, saved).await?;
        }
        Ok(Step::Continue)
    }

    // the items sent upstream: from the latest checkpoint, without reasoning another model wrote
    fn context(&self, history: &[InputItem]) -> Vec<InputItem> {
        let start = history::context_start(history);
        history[start..]
            .iter()
            .enumerate()
            .filter(|(offset, item)| {
                start + offset >= self.reasoning_from || !history::is_reasoning(item)
            })
            .map(|(_, item)| item.clone())
            .collect()
    }

    fn compact_threshold(&self) -> u64 {
        self.model.context_window() * COMPACT_AT_PERCENT / 100
    }

    // a request that overflows the window compacts the context once and tries again
    async fn sample(
        &self,
        history: &mut Vec<InputItem>,
        events: &mpsc::Sender<Event>,
    ) -> anyhow::Result<Sampled> {
        let mut compacted = false;
        loop {
            let context = self.context(history);
            match self.request(context, WORK, events).await {
                Ok(sampled) => return Ok(sampled),
                Err(Failure::ContextOverflow) if !compacted => {
                    tracing::info!(model = self.model.id(), "context overflowed, compacting");
                    self.compact(history, events).await?;
                    compacted = true;
                }
                Err(Failure::ContextOverflow) => {
                    bail!(
                        "the conversation exceeds the model's context window even after compaction"
                    )
                }
                Err(Failure::Retryable(error) | Failure::Fatal(error)) => return Err(error),
            }
        }
    }

    // summarizes the context into a checkpoint that replaces it for every later request
    async fn compact(
        &self,
        history: &mut Vec<InputItem>,
        events: &mpsc::Sender<Event>,
    ) -> anyhow::Result<()> {
        send(events, Event::Compacting).await?;
        let context = &history[history::context_start(history)..];

        // the same prefix as the last turn reuses its prompt cache
        let mut input = self.context(history);
        input.push(history::user_message(SUMMARY_PROMPT));
        let summary = match self.request(input, SUMMARIZE, events).await {
            Ok(sampled) => sampled.text,
            Err(Failure::ContextOverflow) => {
                let budget = self.model.context_window() * TRANSCRIPT_CHARS_PER_WINDOW_TOKEN;
                let transcript = history::transcript(context, budget as usize);
                let prompt =
                    format!("{SUMMARY_PROMPT}\n\n<conversation>\n{transcript}\n</conversation>");
                match self
                    .request(vec![history::user_message(&prompt)], SUMMARIZE, events)
                    .await
                {
                    Ok(sampled) => sampled.text,
                    Err(Failure::ContextOverflow) => {
                        bail!("the conversation is too large to summarize")
                    }
                    Err(Failure::Retryable(error) | Failure::Fatal(error)) => {
                        return Err(error.context("failed to compact the conversation"));
                    }
                }
            }
            Err(Failure::Retryable(error) | Failure::Fatal(error)) => {
                return Err(error.context("failed to compact the conversation"));
            }
        };

        let summary = summary.trim();
        if summary.is_empty() {
            bail!("failed to compact the conversation: the summary was empty");
        }
        let recent = history::recent_user_messages(history, RECENT_USER_MESSAGE_CHARS);
        history.push(history::checkpoint(&recent, summary));
        send(events, Event::Compacted).await?;
        Ok(())
    }

    // transient failures are retried with backoff; the failed attempt had no side effects because
    // tools only run after a response completes
    async fn request(
        &self,
        input: Vec<InputItem>,
        mode: Mode,
        events: &mpsc::Sender<Event>,
    ) -> Result<Sampled, Failure> {
        let input = attachment::resolve(self.library, self.workspace, input).await?;
        let mut attempt = 1;
        loop {
            match self.stream_turn(input.clone(), mode, events).await {
                Err(Failure::Retryable(error)) if attempt < MAX_ATTEMPTS => {
                    let delay = retry_delay(attempt);
                    let reason = format!("{error:#}");
                    tracing::warn!(
                        attempt,
                        delay_secs = delay.as_secs(),
                        error = reason,
                        "retrying model request"
                    );
                    send(
                        events,
                        Event::Retrying {
                            attempt,
                            max_attempts: MAX_ATTEMPTS,
                            delay,
                            reason,
                        },
                    )
                    .await?;
                    sleep(delay).await;
                    attempt += 1;
                }
                Err(Failure::Retryable(error)) => {
                    return Err(Failure::Fatal(error.context(format!(
                        "model request failed after {MAX_ATTEMPTS} attempts"
                    ))));
                }
                result => return result,
            }
        }
    }

    // surfaces each network request the sandbox was denied once, so the user can approve it mid-run
    async fn watch_access(&self, events: &mpsc::Sender<Event>) -> anyhow::Error {
        let mut seen = HashSet::new();
        loop {
            sleep(ACCESS_POLL_INTERVAL).await;
            let Some(sandbox) = self.sandbox.get_if_ready() else {
                continue;
            };
            let requests = match self.openshell.pending_access(sandbox).await {
                Ok(requests) => requests,
                Err(error) => {
                    tracing::warn!(
                        error = format!("{error:#}"),
                        "failed to poll access requests"
                    );
                    continue;
                }
            };
            for request in requests {
                if seen.insert(request.id.clone())
                    && let Err(error) = send(events, Event::AccessRequest(request)).await
                {
                    return error;
                }
            }
        }
    }

    async fn stream_turn(
        &self,
        input: Vec<InputItem>,
        mode: Mode,
        events: &mpsc::Sender<Event>,
    ) -> Result<Sampled, Failure> {
        let turn = Turn {
            instructions: self.instructions.clone(),
            input,
            tools: if self.unattended {
                self.tools.definitions()
            } else {
                [
                    self.tools.definitions(),
                    vec![Tool::Function(question::definition())],
                ]
                .concat()
            },
            reasoning_effort: self.reasoning_effort.clone(),
            allow_tools: mode.tools,
            cache_key: self.cache_key.clone(),
            fast: self.fast,
        };
        let started = Instant::now();
        let mut first_token = None;
        let mut stream = match timeout(STREAM_IDLE_TIMEOUT, self.model.stream(turn)).await {
            Ok(Ok(stream)) => stream,
            Ok(Err(error)) => return Err(classify(error)),
            Err(_) => {
                return Err(Failure::Retryable(anyhow!(
                    "timed out starting the response"
                )));
            }
        };

        let mut sampled = Sampled {
            items: Vec::new(),
            calls: Vec::new(),
            text: String::new(),
            context_tokens: 0,
        };
        // argument deltas name the output item, so remember which call each one belongs to
        let mut streaming_calls: HashMap<u32, String> = HashMap::new();
        loop {
            let event = match timeout(STREAM_IDLE_TIMEOUT, stream.next()).await {
                Ok(Some(Ok(event))) => event,
                Ok(Some(Err(error))) => return Err(classify(error)),
                Ok(None) => {
                    return Err(Failure::Retryable(anyhow!(
                        "model stream ended before the response completed"
                    )));
                }
                Err(_) => {
                    return Err(Failure::Retryable(anyhow!(
                        "model stream sent nothing for {}s",
                        STREAM_IDLE_TIMEOUT.as_secs()
                    )));
                }
            };
            if first_token.is_none()
                && matches!(
                    event,
                    ResponseStreamEvent::ResponseOutputTextDelta(_)
                        | ResponseStreamEvent::ResponseReasoningSummaryTextDelta(_)
                        | ResponseStreamEvent::ResponseReasoningTextDelta(_)
                        | ResponseStreamEvent::ResponseFunctionCallArgumentsDelta(_)
                )
            {
                first_token = Some(started.elapsed());
            }
            let timing = Timing {
                duration: started.elapsed(),
                first_token,
            };
            match event {
                ResponseStreamEvent::ResponseOutputTextDelta(e) => {
                    sampled.text.push_str(&e.delta);
                    if mode.live {
                        send(events, Event::TextDelta { delta: e.delta }).await?;
                    }
                }
                ResponseStreamEvent::ResponseReasoningSummaryTextDelta(e) => {
                    if mode.live {
                        send(events, Event::ReasoningDelta { delta: e.delta }).await?;
                    }
                }
                ResponseStreamEvent::ResponseOutputItemAdded(e) => {
                    if let (OutputItem::FunctionCall(call), true) = (e.item, mode.live) {
                        streaming_calls.insert(e.output_index, call.call_id.clone());
                        send(
                            events,
                            Event::ToolCallStarted {
                                call_id: call.call_id,
                                name: call.name,
                            },
                        )
                        .await?;
                    }
                }
                ResponseStreamEvent::ResponseFunctionCallArgumentsDelta(e) => {
                    if let Some(call_id) = streaming_calls.get(&e.output_index) {
                        let event = Event::ToolCallDelta {
                            call_id: call_id.clone(),
                            delta: e.delta,
                        };
                        send(events, event).await?;
                    }
                }
                ResponseStreamEvent::ResponseOutputItemDone(e) => match e.item {
                    OutputItem::FunctionCall(call) => {
                        send(
                            events,
                            Event::ToolCall {
                                call_id: call.call_id.clone(),
                                name: call.name.clone(),
                                arguments: call.arguments.clone(),
                            },
                        )
                        .await?;
                        sampled.items.push(Item::FunctionCall(call.clone()).into());
                        sampled.calls.push(call);
                    }
                    // anything else, including provider-specific items, is replayed verbatim
                    item => sampled.items.push(item.into()),
                },
                ResponseStreamEvent::ResponseCompleted(e) => {
                    self.report_usage(&e.response, timing, events).await?;
                    sampled.context_tokens = e.response.usage.map_or(0, |usage| {
                        u64::from(usage.input_tokens) + u64::from(usage.output_tokens)
                    });
                    return Ok(sampled);
                }
                ResponseStreamEvent::ResponseIncomplete(e) => {
                    self.report_usage(&e.response, timing, events).await?;
                    let reason = e.response.incomplete_details.map(|d| d.reason);
                    return Err(Failure::Fatal(anyhow!(
                        "model response incomplete: {}",
                        reason.unwrap_or_default()
                    )));
                }
                ResponseStreamEvent::ResponseFailed(e) => {
                    self.report_usage(&e.response, timing, events).await?;
                    let Some(error) = e.response.error else {
                        return Err(Failure::Retryable(anyhow!("model response failed")));
                    };
                    let code = match error.code {
                        ResponseErrorCode::ServerError => "server_error".to_owned(),
                        ResponseErrorCode::RateLimitExceeded => "rate_limit_exceeded".to_owned(),
                        ResponseErrorCode::Other(code) => code,
                        other => format!("{other:?}"),
                    };
                    return Err(classify_code(
                        Some(&code),
                        &error.message,
                        anyhow!("model response failed: {}", error.message),
                    ));
                }
                ResponseStreamEvent::ResponseError(e) => {
                    return Err(classify_code(
                        e.code.as_deref(),
                        &e.message,
                        anyhow!("model error: {}", e.message),
                    ));
                }
                _ => {}
            }
        }
    }

    // incomplete and failed responses still consume tokens, so every terminal response is metered
    async fn report_usage(
        &self,
        response: &Response,
        timing: Timing,
        events: &mpsc::Sender<Event>,
    ) -> anyhow::Result<()> {
        let Some(usage) = &response.usage else {
            tracing::warn!(model = self.model.id(), "response has no usage");
            return Ok(());
        };
        let usage = Usage {
            model: self.model.id().to_owned(),
            input_tokens: usage.input_tokens.into(),
            cached_input_tokens: usage.input_tokens_details.cached_tokens.into(),
            cache_write_tokens: usage
                .input_tokens_details
                .cache_write_tokens
                .map_or(0, |tokens| tokens.max(0) as u64),
            output_tokens: usage.output_tokens.into(),
            reasoning_tokens: usage.output_tokens_details.reasoning_tokens.into(),
            duration: timing.duration,
            time_to_first_token: timing.first_token,
        };
        send(events, Event::Usage(usage)).await
    }

    // tool failures are reported to the model as output so it can recover
    async fn call_tool(
        &self,
        call: &FunctionToolCall,
        events: &mpsc::Sender<Event>,
    ) -> anyhow::Result<ToolOutput> {
        // only sandbox commands reach the network through openshell, so only they wait for access
        let gate = (call.name == BASH).then_some(AccessGate {
            openshell: self.openshell,
            sandbox: self.sandbox,
            unattended: self.unattended,
        });
        let before = match &gate {
            Some(gate) => gate.pending().await,
            None => HashSet::new(),
        };
        let ctx = ToolContext {
            workspace: self.workspace,
            library: self.library,
            openshell: self.openshell,
            sandbox: self.sandbox,
            call_id: &call.call_id,
            events,
        };

        let (output, is_error) = match self
            .tools
            .call_content(ctx, &call.name, &call.arguments)
            .await
        {
            Ok(output) => (output, false),
            Err(error) => (ToolOutput::Text(format!("error: {error:#}")), true),
        };
        let output = if self.sandbox.take_replaced() {
            output.prepend(SANDBOX_REPLACED_NOTE)
        } else {
            output
        };
        let output = match &gate {
            Some(gate) => {
                let failed = is_error || !output.text().trim_end().ends_with("[exit code 0]");
                match gate.settle(&before, failed, events).await {
                    Ok(Some(note)) => output.append(&note),
                    Ok(None) => output,
                    Err(error) => {
                        // the command's own output still tells the model it failed
                        tracing::warn!(
                            error = format!("{error:#}"),
                            "failed to check access requests"
                        );
                        output
                    }
                }
            }
            None => output,
        };
        tracing::debug!(
            call_id = call.call_id,
            name = call.name,
            is_error,
            "tool finished"
        );

        send(
            events,
            Event::ToolResult {
                call_id: call.call_id.clone(),
                output: output.text(),
                is_error,
            },
        )
        .await?;
        Ok(output)
    }
}

async fn interrupted(interrupts: Option<watch::Receiver<()>>) {
    // without steering, or once the sender is gone, nothing can interrupt
    if let Some(mut interrupts) = interrupts
        && interrupts.changed().await.is_ok()
    {
        return;
    }
    std::future::pending().await
}

fn retry_delay(attempt: u32) -> Duration {
    RETRY_BASE_DELAY
        .saturating_mul(2u32.saturating_pow(attempt - 1))
        .min(RETRY_MAX_DELAY)
}

fn classify(error: OpenAIError) -> Failure {
    match error {
        OpenAIError::ApiError(response) => {
            let status = response.status_code;
            let api = response.api_error;
            let error = anyhow!("model request failed: {status} {}", api.message);
            if is_overflow(api.code.as_deref(), &api.message) {
                Failure::ContextOverflow
            } else if status.as_u16() == 408 || status.as_u16() == 429 || status.is_server_error() {
                Failure::Retryable(error)
            } else {
                classify_code(api.code.as_deref(), &api.message, error)
            }
        }
        OpenAIError::Reqwest(error) if error.is_builder() => {
            Failure::Fatal(anyhow!(error).context("invalid model request"))
        }
        error @ (OpenAIError::Reqwest(_)
        | OpenAIError::StreamError(_)
        | OpenAIError::JSONDeserialize(..)) => {
            Failure::Retryable(anyhow!(error).context("model connection failed"))
        }
        error => Failure::Fatal(anyhow!(error).context("model request failed")),
    }
}

fn classify_code(code: Option<&str>, message: &str, error: anyhow::Error) -> Failure {
    if is_overflow(code, message) {
        return Failure::ContextOverflow;
    }
    match code {
        Some("server_error" | "rate_limit_exceeded" | "server_is_overloaded" | "slow_down") => {
            Failure::Retryable(error)
        }
        _ => Failure::Fatal(error),
    }
}

// providers without the openai error code still say so in the message
fn is_overflow(code: Option<&str>, message: &str) -> bool {
    let message = message.to_lowercase();
    code == Some("context_length_exceeded")
        || message.contains("context_length_exceeded")
        || message.contains("maximum context length")
        || message.contains("exceeds the context window")
}

pub(crate) fn output_item(call_id: String, output: String) -> InputItem {
    content_output_item(call_id, ToolOutput::Text(output))
}

fn content_output_item(call_id: String, output: ToolOutput) -> InputItem {
    InputItem::Item(Item::FunctionCallOutput(FunctionCallOutputItemParam {
        call_id: Some(call_id),
        output: output.into_function_output(),
        id: None,
        status: None,
        caller: None,
        name: None,
        namespace: None,
    }))
}

async fn send(events: &mpsc::Sender<Event>, event: Event) -> anyhow::Result<()> {
    events.send(event).await.context("event receiver dropped")
}

#[cfg(test)]
mod tests {
    use std::{
        collections::VecDeque,
        sync::{Arc, Mutex},
    };

    use async_openai::types::responses::EasyInputMessage;
    use axum::{
        Router,
        body::Bytes,
        extract::State,
        http::{HeaderMap, StatusCode, header},
        response::{IntoResponse, Response as HttpResponse},
        routing::post,
    };
    use serde_json::{Value, json};

    use super::*;
    use crate::{
        model::{TEST_MODEL, test_base_url, test_model_via, test_models},
        question::{Answer, Question},
        test_support::sandbox_for_new_user,
        tool::Tools,
    };

    #[test]
    fn classifies_model_errors() {
        let error = || anyhow!("failed");
        assert!(matches!(
            classify_code(Some("context_length_exceeded"), "", error()),
            Failure::ContextOverflow
        ));
        assert!(matches!(
            classify_code(
                None,
                "This model's maximum context length is 8192 tokens",
                error()
            ),
            Failure::ContextOverflow
        ));
        assert!(matches!(
            classify_code(Some("server_error"), "oops", error()),
            Failure::Retryable(_)
        ));
        assert!(matches!(
            classify_code(Some("invalid_prompt"), "no", error()),
            Failure::Fatal(_)
        ));
        assert!(matches!(
            classify_code(None, "", error()),
            Failure::Fatal(_)
        ));
    }

    #[test]
    fn backs_off_exponentially_up_to_a_cap() {
        let delays: Vec<u64> = (1..=7).map(|n| retry_delay(n).as_secs()).collect();
        assert_eq!(delays, [2, 4, 8, 16, 32, 60, 60]);
    }

    #[derive(Clone, Copy)]
    enum Fault {
        DropStream,
        Overflow,
    }

    struct Proxy {
        upstream: String,
        client: reqwest::Client,
        faults: Mutex<VecDeque<Option<Fault>>>,
        requests: Mutex<Vec<Value>>,
    }

    // stands in for the model provider: each request takes the next fault, and with none left it
    // forwards to the real endpoint from trex.toml
    async fn start_proxy(upstream: String, faults: Vec<Option<Fault>>) -> (String, Arc<Proxy>) {
        let proxy = Arc::new(Proxy {
            upstream,
            client: reqwest::Client::new(),
            faults: Mutex::new(faults.into()),
            requests: Mutex::new(Vec::new()),
        });
        let app = Router::new()
            .route("/v1/responses", post(proxy_response))
            .with_state(proxy.clone());
        let listener = tokio::net::TcpListener::bind("127.0.0.1:0").await.unwrap();
        let url = format!("http://{}/v1", listener.local_addr().unwrap());
        tokio::spawn(async move { axum::serve(listener, app).await.unwrap() });
        (url, proxy)
    }

    async fn proxy_response(
        State(proxy): State<Arc<Proxy>>,
        headers: HeaderMap,
        body: Bytes,
    ) -> HttpResponse {
        proxy
            .requests
            .lock()
            .unwrap()
            .push(serde_json::from_slice(&body).unwrap());
        let fault = proxy.faults.lock().unwrap().pop_front().flatten();
        let sse = [(header::CONTENT_TYPE, "text/event-stream")];
        match fault {
            Some(Fault::DropStream) => (sse, "").into_response(),
            Some(Fault::Overflow) => {
                let error = json!({"type": "error", "sequence_number": 0, "code": "context_length_exceeded", "message": "Your input exceeds the context window of this model.", "param": "input"});
                (sse, format!("event: error\ndata: {error}\n\n")).into_response()
            }
            None => {
                let mut request = proxy
                    .client
                    .post(format!("{}/responses", proxy.upstream))
                    .header(header::CONTENT_TYPE, "application/json")
                    .body(body);
                if let Some(auth) = headers.get(header::AUTHORIZATION) {
                    request = request.header(header::AUTHORIZATION, auth);
                }
                let upstream = request.send().await.unwrap();
                let status = StatusCode::from_u16(upstream.status().as_u16()).unwrap();
                (status, sse, upstream.text().await.unwrap()).into_response()
            }
        }
    }

    fn drain(rx: &mut mpsc::Receiver<Event>) -> (String, Vec<u32>, usize, usize) {
        let (mut text, mut retries, mut compacting, mut compacted) =
            (String::new(), Vec::new(), 0, 0);
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::TextDelta { delta } => text.push_str(&delta),
                Event::Retrying { attempt, .. } => {
                    text.clear();
                    retries.push(attempt);
                }
                Event::Compacting => compacting += 1,
                Event::Compacted => compacted += 1,
                _ => {}
            }
        }
        (text, retries, compacting, compacted)
    }

    fn is_checkpoint(item: &InputItem) -> bool {
        history::checkpoint_text(&serde_json::to_value(item).unwrap()).is_some()
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn recovers_from_dropped_streams_and_context_overflow() {
        let faults = vec![Some(Fault::DropStream), None, Some(Fault::Overflow)];
        let (url, proxy) = start_proxy(test_base_url(), faults).await;
        let model = test_model_via(&url, 400_000);
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let tools = Tools::standard().unwrap();
        let agent = Agent {
            workspace: user,
            library: &library,
            model: &model,
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: None,
            reasoning_effort: Some(ReasoningEffort::Low),
            max_turns: 5,
            cache_key: None,
            steering: None,
            journal: None,
            fast: false,
            budget: None,
            reasoning_from: 0,
            unattended: false,
        };
        let (tx, mut rx) = mpsc::channel(1024);

        let mut history = vec![history::user_message(
            "My secret code word is PELICAN-42. Reply with only the word noted.",
        )];
        let first = agent.run(&mut history, &tx).await;
        let (first_text, first_retries, _, _) = drain(&mut rx);

        history.push(history::user_message(
            "What is my secret code word? Reply with only the code word.",
        ));
        let second = agent.run(&mut history, &tx).await;
        let (second_text, second_retries, compacting, compacted) = drain(&mut rx);
        openshell.delete_workspace(user).await.unwrap();

        assert_eq!(first.unwrap(), RunOutcome::Completed);
        assert_eq!(first_retries, [1]);
        assert!(first_text.to_lowercase().contains("noted"), "{first_text}");

        assert_eq!(second.unwrap(), RunOutcome::Completed);
        assert!(second_retries.is_empty());
        assert_eq!((compacting, compacted), (1, 1));
        assert!(second_text.contains("PELICAN-42"), "{second_text}");

        let start = history::context_start(&history);
        assert!(start > 0 && is_checkpoint(&history[start]));
        let requests = proxy.requests.lock().unwrap();
        assert_eq!(requests.len(), 5);
        assert_eq!(requests[3]["tool_choice"], "none");
        let last = &requests[4]["input"];
        assert_eq!(last.as_array().unwrap().len(), 1);
        assert_eq!(last[0]["role"], "developer");
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, the dev image, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn compacts_when_the_context_fills_up_and_keeps_working() {
        let (url, proxy) = start_proxy(test_base_url(), Vec::new()).await;
        // tiny, so every tool turn crosses the threshold and the task spans several checkpoints
        let model = test_model_via(&url, 1_000);
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let tools = Tools::standard().unwrap();
        let agent = Agent {
            workspace: user,
            library: &library,
            model: &model,
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: None,
            reasoning_effort: Some(ReasoningEffort::Low),
            max_turns: 10,
            cache_key: None,
            steering: None,
            journal: None,
            fast: false,
            budget: None,
            reasoning_from: 0,
            unattended: false,
        };
        let (tx, mut rx) = mpsc::channel(1024);

        let mut history = vec![history::user_message(
            "Use bash to write the word trex into /tmp/name.txt. After that call finishes, use read_file \
             in a separate call to read it back, then reply with only the file's contents.",
        )];
        let result = agent.run(&mut history, &tx).await;
        let (text, _, compacting, compacted) = drain(&mut rx);
        openshell.delete_workspace(user).await.unwrap();

        assert_eq!(result.unwrap(), RunOutcome::Completed);
        assert_eq!(text.trim(), "trex");
        assert!(compacted >= 1 && compacting == compacted);
        assert!(history.iter().any(is_checkpoint));
        let requests = proxy.requests.lock().unwrap();
        let summaries = requests
            .iter()
            .filter(|request| request["tool_choice"] == "none")
            .count();
        assert_eq!(summaries, compacted);
    }

    // hands out one scripted batch of messages per take
    struct ScriptedInbox(Mutex<VecDeque<Vec<String>>>);

    impl Inbox for ScriptedInbox {
        fn take(&self) -> BoxFuture<'_, anyhow::Result<Vec<Value>>> {
            let batch = self.0.lock().unwrap().pop_front().unwrap_or_default();
            let items: Vec<_> = batch
                .iter()
                .map(|text| history::user_message(text))
                .collect();
            Box::pin(async move { history::to_json(&items) })
        }
    }

    fn user_messages(history: &[InputItem]) -> Vec<String> {
        history
            .iter()
            .filter_map(|item| {
                let value = serde_json::to_value(item).unwrap();
                (value["role"] == "user").then(|| value["content"].as_str().unwrap().to_owned())
            })
            .collect()
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn reads_messages_that_arrive_as_it_finishes() {
        let models = test_models();
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let tools = Tools::standard().unwrap();
        let inbox = ScriptedInbox(Mutex::new(
            [vec![], vec!["Now reply with only the word two.".into()]].into(),
        ));
        let (_interrupt, interrupts) = watch::channel(());
        let agent = Agent {
            workspace: user,
            library: &library,
            model: models.get(TEST_MODEL).unwrap(),
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: None,
            reasoning_effort: Some(ReasoningEffort::Low),
            max_turns: 5,
            cache_key: None,
            steering: Some(Steering {
                inbox: &inbox,
                interrupts,
            }),
            journal: None,
            fast: false,
            budget: None,
            reasoning_from: 0,
            unattended: false,
        };
        let (tx, mut rx) = mpsc::channel(1024);
        let mut history = vec![history::user_message("Reply with only the word one.")];

        let result = agent.run(&mut history, &tx).await;
        openshell.delete_workspace(user).await.unwrap();

        let mut received = Vec::new();
        let mut text = String::new();
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::MessageReceived { content } => received.push(content),
                Event::TextDelta { delta } => text.push_str(&delta),
                _ => {}
            }
        }
        assert_eq!(result.unwrap(), RunOutcome::Completed);
        assert_eq!(received, ["Now reply with only the word two."]);
        assert_eq!(
            user_messages(&history),
            [
                "Reply with only the word one.",
                "Now reply with only the word two."
            ]
        );
        assert!(text.to_lowercase().trim_end().ends_with("two"), "{text}");
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, the dev image, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn interrupts_a_running_tool_to_read_a_message() {
        let models = test_models();
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let tools = Tools::standard().unwrap();
        let inbox = ScriptedInbox(Mutex::new(VecDeque::new()));
        let (interrupt, interrupts) = watch::channel(());
        let agent = Agent {
            workspace: user,
            library: &library,
            model: models.get(TEST_MODEL).unwrap(),
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: None,
            reasoning_effort: Some(ReasoningEffort::Low),
            max_turns: 5,
            cache_key: None,
            steering: Some(Steering {
                inbox: &inbox,
                interrupts,
            }),
            journal: None,
            fast: false,
            budget: None,
            reasoning_from: 0,
            unattended: false,
        };
        let (tx, mut rx) = mpsc::channel(1024);
        let mut history = vec![history::user_message(
            "Use bash to run `sleep 100 && echo finished`, then reply with what it printed.",
        )];

        let (mut called, mut sent, mut interrupted) = (false, false, 0);
        let (mut sent_at, mut reaction) = (None, None);
        let mut text = String::new();
        let result = {
            let run = agent.run(&mut history, &tx);
            tokio::pin!(run);
            loop {
                tokio::select! {
                    result = &mut run => break result,
                    Some(event) = rx.recv() => match event {
                        Event::ToolCall { .. } => called = true,
                        // usage marks the end of the response, so the tool is about to run
                        Event::Usage(_) if called && !sent => {
                            sent = true;
                            // the inbox is filled before the signal, like the api does
                            inbox.0.lock().unwrap().push_back(vec![
                                "Stop that, I changed my mind. Reply with only the word stopped.".into(),
                            ]);
                            interrupt.send_replace(());
                            sent_at = Some(Instant::now());
                        }
                        Event::Interrupted => {
                            interrupted += 1;
                            reaction = sent_at.map(|at| at.elapsed());
                        }
                        Event::TextDelta { delta } => text.push_str(&delta),
                        _ => {}
                    },
                }
            }
        };
        openshell.delete_workspace(user).await.unwrap();

        assert_eq!(result.unwrap(), RunOutcome::Completed);
        assert_eq!(interrupted, 1);
        assert!(
            reaction.is_some_and(|r| r < Duration::from_secs(5)),
            "the sleep was not cut short: {reaction:?}"
        );
        assert!(text.to_lowercase().contains("stopped"), "{text}");
        let outputs: Vec<String> = history
            .iter()
            .filter_map(|item| match item {
                InputItem::Item(Item::FunctionCallOutput(output)) => {
                    serde_json::to_value(&output.output).ok()
                }
                _ => None,
            })
            .map(|output| output.to_string())
            .collect();
        assert!(
            outputs.iter().any(|output| output.contains("interrupted")),
            "{outputs:?}"
        );
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn runs_bash_in_sandbox() {
        let models = test_models();
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let tools = Tools::standard().unwrap();

        let library = Library::in_memory();
        let agent = Agent {
            workspace: user,
            library: &library,
            model: models.get(TEST_MODEL).unwrap(),
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: Some("You are a coding agent. Use the bash tool to act.".into()),
            reasoning_effort: Some(ReasoningEffort::Low),
            max_turns: 10,
            cache_key: None,
            steering: None,
            journal: None,
            fast: false,
            budget: None,
            reasoning_from: 0,
            unattended: false,
        };

        let mut history = vec![
            EasyInputMessage::from(
                "Write the word trex into /tmp/name.txt, then read the file back and reply with only its contents.",
            )
            .into(),
        ];
        let (tx, mut rx) = mpsc::channel(256);
        let collector = tokio::spawn(async move {
            let (mut text, mut tool_results, mut done) = (String::new(), 0, false);
            let mut usages = Vec::new();
            while let Some(event) = rx.recv().await {
                match event {
                    Event::TextDelta { delta } => text.push_str(&delta),
                    Event::ToolResult { .. } => tool_results += 1,
                    Event::Usage(usage) => usages.push(usage),
                    Event::Done => done = true,
                    _ => {}
                }
            }
            (text, tool_results, done, usages)
        });

        let result = agent.run(&mut history, &tx).await;
        drop(tx);
        let (text, tool_results, done, usages) = collector.await.unwrap();
        openshell.delete_workspace(user).await.unwrap();

        result.unwrap();
        assert!(done);
        assert!(tool_results >= 1);
        assert!(
            usages.len() >= 2,
            "a tool turn and a final turn are both metered"
        );
        assert!(
            usages
                .iter()
                .all(|u| u.model == TEST_MODEL && u.input_tokens > 0)
        );
        assert_eq!(text.trim(), "trex");
        assert!(
            history
                .iter()
                .any(|item| matches!(item, InputItem::Item(Item::FunctionCallOutput(_))))
        );
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn replays_encrypted_reasoning() {
        let models = test_models();
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let tools = Tools::standard().unwrap();

        let library = Library::in_memory();
        let agent = Agent {
            workspace: user,
            library: &library,
            model: models.get(TEST_MODEL).unwrap(),
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: None,
            reasoning_effort: Some(ReasoningEffort::High),
            max_turns: 10,
            cache_key: None,
            steering: None,
            journal: None,
            fast: false,
            budget: None,
            reasoning_from: 0,
            unattended: false,
        };
        let mut history = vec![
            EasyInputMessage::from(
                "How many primes are below 60? Work it out, verify it with the bash tool, then reply with only the number.",
            )
            .into(),
        ];
        let (tx, mut rx) = mpsc::channel(256);
        let collector = tokio::spawn(async move {
            let mut text = String::new();
            while let Some(event) = rx.recv().await {
                if let Event::TextDelta { delta } = event {
                    text.push_str(&delta);
                }
            }
            text
        });

        let result = agent.run(&mut history, &tx).await;
        drop(tx);
        let text = collector.await.unwrap();
        openshell.delete_workspace(user).await.unwrap();

        result.unwrap();
        assert_eq!(text.trim(), "17");
        // the reasoning precedes a tool call, so the follow-up turn resent it and was accepted
        let reasoning = history.iter().position(|item| {
            matches!(item, InputItem::Item(Item::Reasoning(r)) if r.encrypted_content.is_some())
        });
        let call = history
            .iter()
            .position(|item| matches!(item, InputItem::Item(Item::FunctionCall(_))));
        assert!(matches!((reasoning, call), (Some(r), Some(c)) if r < c));
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn surfaces_denied_network_access() {
        let models = test_models();
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let tools = Tools::standard().unwrap();
        let library = Library::in_memory();
        let agent = Agent {
            workspace: user,
            library: &library,
            model: models.get(TEST_MODEL).unwrap(),
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: None,
            reasoning_effort: None,
            max_turns: 1,
            cache_key: None,
            steering: None,
            journal: None,
            fast: false,
            budget: None,
            reasoning_from: 0,
            unattended: false,
        };

        let script = "timeout 15 bash -c 'exec 3<>/dev/tcp/example.com/443'";
        let denied = openshell
            .output(
                sandbox.get_if_ready().unwrap(),
                ["bash", "-c", script].map(String::from).to_vec(),
                Vec::new(),
            )
            .await
            .unwrap();

        let (tx, mut rx) = mpsc::channel(16);
        let first = tokio::time::timeout(Duration::from_secs(90), async {
            tokio::select! {
                error = agent.watch_access(&tx) => panic!("watcher stopped: {error:#}"),
                event = rx.recv() => event,
            }
        })
        .await;

        openshell.delete_workspace(user).await.unwrap();

        assert_ne!(denied.exit_code, Some(0));
        let Ok(Some(Event::AccessRequest(request))) = first else {
            panic!("expected an access request event");
        };
        assert!(request.endpoints.iter().any(|e| e == "example.com:443"));
        assert!(!request.review_token.is_empty());
    }

    async fn collect(
        rx: &mut mpsc::Receiver<Event>,
    ) -> (String, Vec<String>, Vec<(String, Vec<Question>)>) {
        let (mut text, mut calls, mut asked) = (String::new(), Vec::new(), Vec::new());
        while let Ok(event) = rx.try_recv() {
            match event {
                Event::TextDelta { delta } => text.push_str(&delta),
                Event::ToolCall {
                    name, arguments, ..
                } => calls.push(format!("{name} {arguments}")),
                Event::Question { call_id, questions } => asked.push((call_id, questions)),
                _ => {}
            }
        }
        (text, calls, asked)
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, the dev image, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn asks_user_and_continues_with_the_answer() {
        let models = test_models();
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let tools = Tools::standard().unwrap();
        let agent = Agent {
            workspace: user,
            library: &library,
            model: models.get(TEST_MODEL).unwrap(),
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: None,
            reasoning_effort: Some(ReasoningEffort::Low),
            max_turns: 10,
            cache_key: None,
            steering: None,
            journal: None,
            fast: false,
            budget: None,
            reasoning_from: 0,
            unattended: false,
        };
        let mut history = vec![
            EasyInputMessage::from(
                "I want a script that prints exactly `hello from trex` with no punctuation. Before writing anything, use ask_user to ask me \
                 which language to use, offering Python and Node as options. Then write it in /sandbox, run it, and \
                 reply with only what it printed.",
            )
            .into(),
        ];
        let (tx, mut rx) = mpsc::channel(1024);

        let first = agent.run(&mut history, &tx).await;
        let (_, _, asked) = collect(&mut rx).await;
        let (call_id, questions) = asked.into_iter().next().expect("the model should ask");
        let python = questions[0]
            .options
            .iter()
            .find(|option| option.label.to_lowercase().contains("python"))
            .map(|option| option.label.clone())
            .expect("python should be offered");
        let answers = [Answer {
            selected: vec![python],
            text: None,
        }];
        history.push(question::answer_item(&call_id, &questions, &answers));

        let second = agent.run(&mut history, &tx).await;
        let (text, calls, _) = collect(&mut rx).await;

        openshell.delete_workspace(user).await.unwrap();

        assert_eq!(first.unwrap(), RunOutcome::NeedsInput);
        assert_eq!(second.unwrap(), RunOutcome::Completed);
        assert!(
            calls.iter().any(|call| call.contains("python")),
            "{calls:?}"
        );
        assert_eq!(text.trim(), "hello from trex");
    }

    // needs the openshell gateway tunnel, <workspace>/certs/openshell, the dev image, and trex.toml
    #[tokio::test]
    #[ignore]
    async fn continues_when_user_ignores_question() {
        let models = test_models();
        let (openshell, user, sandbox) = sandbox_for_new_user().await;
        let sandbox = LazySandbox::ready(sandbox);
        let library = Library::in_memory();
        let tools = Tools::standard().unwrap();
        let agent = Agent {
            workspace: user,
            library: &library,
            model: models.get(TEST_MODEL).unwrap(),
            tools: &tools,
            openshell: &openshell,
            sandbox: &sandbox,
            instructions: None,
            reasoning_effort: Some(ReasoningEffort::Low),
            max_turns: 5,
            cache_key: None,
            steering: None,
            journal: None,
            fast: false,
            budget: None,
            reasoning_from: 0,
            unattended: false,
        };
        let mut history = vec![
            EasyInputMessage::from(
                "Use ask_user to ask me what my favourite colour is. Do nothing else.",
            )
            .into(),
        ];
        let (tx, mut rx) = mpsc::channel(1024);

        let first = agent.run(&mut history, &tx).await;
        history
            .push(EasyInputMessage::from("Never mind that. Reply with only the word ok.").into());
        let second = agent.run(&mut history, &tx).await;
        let (text, _, asked) = collect(&mut rx).await;

        openshell.delete_workspace(user).await.unwrap();

        assert_eq!(first.unwrap(), RunOutcome::NeedsInput);
        assert_eq!(second.unwrap(), RunOutcome::Completed);
        assert_eq!(asked.len(), 1);
        assert!(text.trim().to_lowercase().ends_with("ok"), "{text}");
    }
}
