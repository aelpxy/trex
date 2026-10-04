use trex_sandbox::AccessRequest;

pub enum Event {
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
    Done,
}

pub struct Usage {
    pub model: String,
    pub input_tokens: u64,
    pub cached_input_tokens: u64,
    pub cache_write_tokens: u64,
    pub output_tokens: u64,
    pub reasoning_tokens: u64,
}

#[derive(Clone, Copy)]
pub enum OutputStream {
    Stdout,
    Stderr,
}
