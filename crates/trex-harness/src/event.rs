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
    Done,
}

#[derive(Clone, Copy)]
pub enum OutputStream {
    Stdout,
    Stderr,
}
