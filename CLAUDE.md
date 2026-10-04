# trex

Agentic harness exposed as an HTTP API (not a CLI). trex runs the model loop and sessions; tools execute inside OpenShell sandboxes.

## Architecture

- **API**: axum, JSON in, SSE out. Runs are decoupled from client connections; streams are resumable via event ids (`Last-Event-ID`) with heartbeats.
- **Model**: OpenAI Responses API with `store=false`. trex owns all conversation state; output items (including encrypted reasoning and compaction items) are persisted and sent back verbatim.
- **Sandbox**: OpenShell gateway over gRPC + mTLS via `openshell-sdk` (git dep pinned to the gateway's version tag). One sandbox per session.

## Commands

- `cargo build` / `cargo run`
- `cargo clippy --all-targets` must be warning-free
- `cargo fmt` before committing
- `cargo test` for unit tests; `cargo test -- --ignored` for tests that need a live OpenShell gateway

## Config

All config comes from `TREX_*` env vars, parsed once in `src/config.rs` into `Config`. Never read env vars elsewhere.

- `TREX_ADDR` (default `127.0.0.1:8080`)
- `TREX_LOG_FORMAT` = `text` | `json`; `RUST_LOG` overrides filters
- `TREX_OPENSHELL_ENDPOINT` (default `https://127.0.0.1:17670`)
- `TREX_OPENSHELL_TLS_DIR` (default `certs/openshell`, relative to the working dir, containing `ca.crt`, `tls.crt`, `tls.key`; `certs/` is gitignored)

Dev setup: the gateway on `fedora-server` only listens on loopback; tunnel with `ssh -fN -L 17670:127.0.0.1:17670 fedora-server`.

## Rust practices

### Structure
- One module per concern (`api`, `config`, `logging`, `openshell`, ...). Split into a directory module only when a file grows past one clear responsibility.
- Wrap external SDKs behind a small trex-owned type (like `OpenShell`) that exposes only what we use. Don't leak SDK/proto types through the rest of the codebase.
- Introduce a trait only when there is a second implementation or a real test seam, not speculatively.
- Keep handlers thin: parse input, call into a domain module, map the result to a response.

### Errors
- `anyhow::Result` with `.context(...)` for application code and startup paths. Context messages are lowercase and say what was being attempted (`"failed to read {path}"`).
- Use a `thiserror` enum where callers need to branch on the error, e.g. mapping to HTTP status codes in the API layer.
- No `unwrap()`/`expect()` outside tests and truly impossible states; `expect` messages state the invariant.
- Never swallow errors silently. Either propagate, or log with `tracing::warn!/error!` and explain why continuing is safe.

### Async
- Everything runs on tokio. Never block the runtime: no `std::thread::sleep`, no blocking I/O in async fns; use `tokio::fs`/`spawn_blocking` when needed.
- Every long-running task must be cancellable (a `CancellationToken` or dropped handle) and have a timeout on external calls.
- Don't hold a lock across an `.await`.
- Prefer channels (`mpsc`/`broadcast`) for streaming events between the agent loop and SSE subscribers over shared mutable state.

### Types
- Make invalid states unrepresentable: enums over strings/bools for modes (see `LogFormat`), newtypes for ids (`SessionId`, `SandboxName`) once they cross module boundaries.
- Derive only what's needed. Don't derive `Debug` on structs holding secrets; implement it manually with redaction.
- Serde types for external APIs tolerate unknown fields and unknown enum variants (keep raw `serde_json::Value` for items we don't model) so upstream additions don't break us.
- Borrow (`&str`, `&Path`) in function parameters; take ownership only when storing.

### Logging
- Use `tracing` with structured fields, not string interpolation: `tracing::info!(sandbox = %name, "created sandbox")`.
- Messages are short lowercase phrases. Put variable data in fields.
- Never log secrets, API keys, tokens, or full model prompts at `info` or above.
- Request spans carry `request_id`; add spans (`session_id`, `run_id`) at boundaries so nested logs inherit them.

### Dependencies
- Add deps with `cargo add`, enabling only the features we use.
- Check before adding: prefer the standard library or an existing dep. Keep one version of tonic/hyper/rustls in the tree (`cargo tree -d`).
- Git deps must be pinned to a tag or rev.

### Testing
- Unit tests live next to the code in `#[cfg(test)] mod tests`.
- Tests that need external services (OpenShell, OpenAI) are `#[ignore]` with a comment saying what they need.
- Test behavior through public functions; assert on concrete values, not just `is_ok()`.

### Style
- `rustfmt` defaults, clippy clean. Fix lints rather than `#[allow]`; when an allow is necessary, scope it narrowly and add a one-line reason.
- Comments only when the why isn't obvious from the code; one line.
- Imports grouped: std, external crates, `crate::`.
- Constants for magic values (timeouts, defaults), named for what they mean.
