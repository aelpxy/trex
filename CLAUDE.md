# trex

Backend and agent harness for a web UI. trex owns sessions, the model catalog the UI picks from (models are configurable per session), the agent loop, and tool execution inside OpenShell sandboxes. The UI only renders and sends input.

## Architecture

- **API**: axum, JSON in, SSE out. Runs are decoupled from client connections; streams are resumable via event ids (`Last-Event-ID`) with heartbeats.
- **Model**: OpenAI Responses API via `async-openai` (no hand-rolled client), `store=false`. trex owns all conversation state; output items (including encrypted reasoning and compaction items) are persisted and sent back verbatim. Any provider speaking the Responses API works.
- **Sandbox**: OpenShell gateway over gRPC + mTLS via `openshell-sdk` (git dep pinned to the gateway's version tag). One sandbox per session, created lazily: only when the agent first calls a tool that needs it (`sandbox::LazySandbox`), so plain conversations never start one. Sandboxes idle for `TREX_SANDBOX_IDLE_SECS` are stopped (files kept) and started again on next use.

## Workspace

- `crates/trex-harness`: the brain. Agent loop, runs, events, tools, model catalog (`model::Models`). Current focus.
- `crates/trex-sandbox`: OpenShell client (`OpenShell`). One OpenShell workspace per trex user (`workspace_name(uuid)` = `u-` + 17 hex chars of sha256, labelled `trex-user=<uuid>` and verified on every `ensure_workspace`). Every call takes a `Sandbox { workspace, name }` handle. trex's mTLS identity is a gateway platform admin, so trex is what enforces tenancy: never build a `Sandbox` for a user from anything but their own workspace. Exec uses the streaming RPC: stdin is chunked under the 1 MiB gRPC limit and dropping the stream kills the process.
- Sandbox image: `images/sandbox/Dockerfile` (Ubuntu 24.04 + Python/uv, Node, Go, Rust, micromamba, build tools). Build it with the gateway's engine on the gateway host: `podman build -t localhost/trex-sandbox:latest images/sandbox`. OpenShell forces `no_new_privs` and a non-root user, so apt never works inside a sandbox; the agent installs software via pip/uv, npm, go, cargo, or micromamba (conda-forge) into `HOME=/sandbox`. TLS is intercepted with a per-sandbox CA exported via `SSL_CERT_FILE` at a driver-specific path; tools that ignore it need a wrapper (see micromamba).
- Sandbox network policy: `sandbox-policy.yaml` (committed, OpenShell's own format, parsed with the `openshell-policy` crate) is the default for session sandboxes. Unlisted egress is denied; OpenShell turns denials into pending access requests (`pending_access`, polled during runs and emitted as `Event::AccessRequest`) that the user approves or rejects (`approve_access` / `reject_access`). Gotchas: an empty `binaries` list matches nothing at enforcement time (use `path: "/**"`), and L7 endpoints default to audit-only unless `enforcement: enforce`.
- `crates/trex-store`: Postgres (sqlx, migrations in `crates/trex-store/migrations`, applied on startup), Redis (connection manager), and the per-user file library (`object_store`, any S3-compatible provider, keyed `users/{uuid}/library/...`). Users, sessions and the credit ledger will live here.
- `crates/trex-server`: the `trex` binary. Config loading, logging, axum API. Stays thin; logic belongs in the harness.

Shared dependency versions live in the root `[workspace.dependencies]`; crates opt into features.

## API design

The HTTP API takes the best of OpenAI, Anthropic and Stripe:

- **Resources** (Stripe): plural nouns (`/v1/sessions/{id}/runs`), prefixed ids (`sess_`, `run_`, `evt_`), an `object` field on every resource, `metadata` map on user-facing objects.
- **Errors** (Stripe/Anthropic): one shape everywhere, `{"error": {"type", "code", "message", "param"}}`, with the `request_id` header on every response. Types are a small fixed set mapped to HTTP status.
- **Lists** (Stripe): cursor pagination with `limit`, `starting_after`, `ending_before`; responses are `{"object": "list", "data": [...], "has_more"}`.
- **Writes** (Stripe): `Idempotency-Key` header on POSTs.
- **Versioning** (Anthropic/Stripe): `/v1` path plus a date-based `Trex-Version` header for breaking changes.
- **Streaming** (OpenAI/Anthropic): SSE with typed events (`event:` name equals the payload's `type`), a monotonically increasing `sequence_number` used as the SSE id for `Last-Event-ID` resume, and explicit start/delta/done events per content block.
- **Content** (OpenAI items/Anthropic blocks): conversations are lists of typed items (message, tool call, tool result, reasoning) so the UI renders by `type`.

## Commands

- `cargo build` / `cargo run` (binary `trex`, run from the workspace root)
- `cargo clippy --workspace --all-targets` must be warning-free
- `cargo fmt` before committing
- `cargo test --workspace` for unit tests; `cargo test --workspace -- --ignored` for tests needing the OpenShell gateway, the Responses API from `trex.toml`, and Postgres/Redis (from `.env`)

## Config

All config is loaded once in `crates/trex-server/src/config.rs` into `Config`: env vars (`.env` in the working directory is loaded first; real environment variables win) and `trex.toml`. Never read env vars or config files elsewhere; library crates take config as plain values. `.env.example` lists every variable.

`trex.toml` (path overridable with `TREX_CONFIG`) is the operator-managed model catalog, parsed by `trex_harness::model::Models::from_toml`. It contains provider api keys, so it is gitignored; `trex.toml.example` is the committed template:

```toml
[providers.local]
base_url = "http://127.0.0.1:8699/v1"
api_key = "..."

[[models]]
id = "gpt-6.1-sol"      # what the UI selects
name = "GPT 6.1 Sol"    # optional, display name for pickers, defaults to id
provider = "local"
upstream = "..."        # optional, model name sent upstream, defaults to id
context_window = 400000 # optional, tokens, defaults to 128000; compaction starts at 80%
```

Env vars:

- `TREX_ADDR` (default `127.0.0.1:8080`)
- `TREX_LOG_FORMAT` = `text` | `json`; `RUST_LOG` overrides filters
- `TREX_CONFIG` (default `trex.toml`)
- `TREX_SANDBOX_IMAGE` (default `localhost/trex-sandbox:latest`), `TREX_SANDBOX_POLICY` (default `sandbox-policy.yaml`), `TREX_SANDBOX_IDLE_SECS` (default `300`)
- `TREX_LIBRARY_DIR` (default `data/library`, gitignored) or `TREX_S3_BUCKET` + `TREX_S3_ENDPOINT`/`_REGION`/`_ACCESS_KEY_ID`/`_SECRET_ACCESS_KEY`/`_FORCE_PATH_STYLE` for any S3-compatible provider
- `TREX_DATABASE_URL`, `TREX_REDIS_URL` (required; contain credentials, so never log them or put them in `trex.toml`)
- `TREX_OPENSHELL_ENDPOINT` (default `https://127.0.0.1:17670`)
- `TREX_OPENSHELL_TLS_DIR` (default `certs/openshell`, relative to the working dir, containing `ca.crt`, `tls.crt`, `tls.key`; `certs/` is gitignored)

Dev setup: the gateway on `fedora-server` only listens on loopback; tunnel with `ssh -fN -L 17670:127.0.0.1:17670 fedora-server`.

## HTTP API (v1)

Auth is temporary: every `/v1` request names its user in `X-Trex-User: <uuid>` until registration and api keys exist.

Docs: `GET /docs` (Scalar, loaded from its CDN by `api/docs.html`) renders `GET /openapi.json`, which utoipa generates from the handlers. Every handler has a `#[utoipa::path]` (summary line, `operation_id`, tag, params, responses with `ErrorResponse` for errors) and is registered with `routes!` in `api::routes()`, so the router and the spec can't drift; `spec_documents_every_route` lists the expected paths. Request and response bodies are typed structs deriving `ToSchema`, never `json!`.

- `GET /v1/models`: `{id, name}` in `trex.toml` order
- `POST /v1/sessions` `{model, reasoning_effort?}`, `GET /v1/sessions?limit&starting_after`, `GET|DELETE /v1/sessions/{id}`
- `GET /v1/sessions/{id}/items`: the conversation as trex items (`message`, `tool_call`, `tool_result`, `reasoning`, `compaction`)
- `POST /v1/sessions/{id}/messages` `{content, interrupt?}` starts a run (202); while one is running the message is queued (`queued: true`) and `interrupt` stops the agent's current step to read it
- `POST /v1/sessions/{id}/answers` `{answers: [{selected, text}]}` resumes a `needs_input` session (202); 409 otherwise
- `POST /v1/sessions/{id}/cancel`
- `GET /v1/sessions/{id}/access_requests`, `POST .../access_requests/{request_id}/approve|reject`
- `GET /v1/sessions/{id}/events`: SSE; resumes from `Last-Event-ID`, `?from=start` replays retained events, otherwise starts at the live tail
- `GET /v1/library`, `GET|PUT|DELETE /v1/library/files/{path}`

Events (`event:` equals the payload `type`): `run.started`, `run.resumed`, `sandbox.creating`, `sandbox.starting`, `sandbox.ready`, `text.delta`, `reasoning.delta`, `tool.call`, `tool.output`, `tool.result`, `usage`, `access.requested`, `question`, `model.retrying`, `context.compacting`, `context.compacted`, `message.received`, `run.interrupted`, then one of `run.completed`, `run.needs_input`, `run.cancelled`, `run.failed`.

Agent loop robustness (`crates/trex-harness/src/agent.rs`):

- Model requests are retried with exponential backoff (5 attempts) on dropped or stalled streams (300s idle timeout), 408/429/5xx, and `server_error`/`rate_limit_exceeded` responses, on top of the client's own connection retries. Tools only run after a response completes, so a retry has no side effects; `model.retrying` tells the UI to discard the partial turn.
- Compaction is trex's own, so it works with any provider (native Responses compaction doesn't shrink the context on our endpoint). When a turn's tokens reach 80% of the model's `context_window`, or a request fails with `context_length_exceeded`, the model writes a handoff summary (same prefix with `tool_choice: none`, so it hits the prompt cache; a trimmed text transcript if even that overflows). It is appended as a developer-role checkpoint message (`history::checkpoint`) holding the recent user messages and the summary. History stays append-only: requests send items from the latest checkpoint (`history::context_start`), and the items API shows it as a `compaction` item.
- Every request carries the session id as `prompt_cache_key`.

Steering: messages sent during a run are queued in `sessions.queued_messages`; the agent takes them before every step (`agent::Inbox`) and when it would finish. A run only finishes while the queue is empty (conditional update), so a message sent as it ends continues it; cancelled and failed runs save leftover messages to history. An interrupt (`agent::Steering::interrupts`, in-memory per instance) drops the current step: partial output is discarded, unfinished tool calls are closed as interrupted (tool outputs are saved as each one finishes).

Idle sandboxes (`crates/trex-server/src/idle.rs`): every minute, sessions not running whose `updated_at` is older than the idle timeout have their sandbox stopped and `sessions.sandbox_stopped` set. The row stays locked (`FOR UPDATE SKIP LOCKED`) while stopping, so a run can't start mid-stop; starting always checks the real phase, so the flag is only a hint.

The agent's system prompt is `crates/trex-server/src/instructions.md` (today's date is appended per run). It is model-facing: never name the agent trex in it, or in anything else the model reads; trex is the harness, not the assistant.

Runs are spawned per session (`crates/trex-server/src/runs.rs`): one at a time, enforced by a conditional update in Postgres. Each run holds a lease (`sessions.run_id` + `run_heartbeat_at`, renewed every 10s); every instance sweeps for runs whose heartbeat is older than 30s (a restart or a crashed instance) and resumes them from saved history (`run.resumed`). A run that loses its lease stops without touching the session. History is saved item by item as the agent produces it (`agent::Journal`, keyed by position so a repeated save is a no-op), and stays strictly append-only: dangling tool calls are closed by appending outputs, never inserting. A resumed run whose history already ends with the model's reply just finishes.

## Rust practices

### Structure

- One crate per layer (see Workspace), one module per concern inside it. Split into a directory module only when a file grows past one clear responsibility.
- Use official or well-maintained SDKs instead of hand-writing API clients.
- Wrap the OpenShell SDK behind `trex_sandbox::OpenShell`; don't leak its proto types. `async-openai` types are the harness's model vocabulary and may be used inside `trex-harness`, but never appear in the HTTP API.
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
- Our own serde types for external data tolerate unknown fields and variants (keep raw `serde_json::Value` for items we don't model) so upstream additions don't break us.
- Borrow (`&str`, `&Path`) in function parameters; take ownership only when storing.

### Logging

- Use `tracing` with structured fields, not string interpolation: `tracing::info!(sandbox = %name, "created sandbox")`.
- Messages are short lowercase phrases. Put variable data in fields.
- Never log secrets, API keys, tokens, or full model prompts at `info` or above.
- Request spans carry `request_id`; add spans (`session_id`, `run_id`) at boundaries so nested logs inherit them.

### Dependencies

- Add deps with `cargo add`, enabling only the features we use.
- Check before adding: prefer the standard library or an existing dep. Keep one version of tonic/hyper/rustls in the tree (`cargo tree -d`). Known duplicate: reqwest 0.12 (openshell-sdk) and 0.13 (async-openai).
- Git deps must be pinned to a tag or rev.

### Testing

- Unit tests live next to the code in `#[cfg(test)] mod tests`.
- Tests that need external services (OpenShell, OpenAI) are `#[ignore]` with a comment saying what they need.
- Test behavior through public functions; assert on concrete values, not just `is_ok()`.

### SQL

- UPPERCASE keywords and types (`CREATE TABLE`, `SELECT ... FROM ... WHERE`, `UUID NOT NULL`), lowercase identifiers.
- No comments inside SQL, in migrations or query strings.
- Migrations live in `crates/trex-store/migrations`, are never edited once committed, and are applied on startup.
- Every query on user data filters by `user_id`.

### Style

- `rustfmt` defaults, clippy clean. Fix lints rather than `#[allow]`; when an allow is necessary, scope it narrowly and add a one-line reason.
- Comments only when the why isn't obvious from the code; one line.
- Imports grouped: std, external crates, workspace crates, `crate::`.
- Constants for magic values (timeouts, defaults), named for what they mean.
