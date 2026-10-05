use std::time::Duration;

use serde::Deserialize;
use trex_sandbox::AccessRequest;

use crate::question::Question;

pub enum Event {
    SandboxCreating,
    SandboxStarting,
    SandboxReady,
    TextDelta {
        delta: String,
    },
    ReasoningDelta {
        delta: String,
    },
    ToolCall {
        call_id: String,
        name: String,
        arguments: String,
    },
    ToolOutput {
        call_id: String,
        stream: OutputStream,
        chunk: String,
    },
    ToolResult {
        call_id: String,
        output: String,
        is_error: bool,
    },
    Usage(Usage),
    AccessRequest(AccessRequest),
    Question {
        call_id: String,
        questions: Vec<Question>,
    },
    // the partial output of the failed attempt should be discarded by whoever shows it
    Retrying {
        attempt: u32,
        max_attempts: u32,
        delay: Duration,
        reason: String,
    },
    Compacting,
    Compacted,
    // a tool created, changed or removed a file in the sandbox; `diff` is a unified diff, and
    // `before`/`after` hold the whole file when it is small enough to show
    FileChanged {
        call_id: String,
        path: String,
        change: FileChange,
        diff: String,
        before: Option<String>,
        after: Option<String>,
    },
    PlanUpdated {
        explanation: Option<String>,
        steps: Vec<PlanStep>,
    },
    // a message the user sent mid-run, now appended to history
    MessageReceived {
        content: String,
    },
    // the user cut the turn short; its partial output should be discarded like a retry's
    Interrupted,
    Done,
}

pub struct Usage {
    pub model: String,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
    // from sending the request to the end of the response
    pub duration: Duration,
    // until the first streamed output; none if nothing visible was streamed
    pub time_to_first_token: Option<Duration>,
}

#[derive(Clone, Copy)]
pub enum OutputStream {
    Stdout,
    Stderr,
}

#[derive(Deserialize)]
pub struct PlanStep {
    pub step: String,
    pub status: StepStatus,
}

#[derive(Clone, Copy, Deserialize, PartialEq)]
#[serde(rename_all = "snake_case")]
pub enum StepStatus {
    Pending,
    InProgress,
    Completed,
}

pub enum FileChange {
    Added,
    Updated,
    Deleted,
    Moved { from: String },
}
