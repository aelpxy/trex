# trex

Backend and agent harness for a web UI. trex owns sessions, the model catalog the UI picks from (models are configurable per session), the agent loop, and tool execution inside OpenShell sandboxes. The UI only renders and sends input.

## Architecture

- **API**: axum, JSON in, SSE out. Runs are decoupled from client connections; streams are resumable via event ids (`Last-Event-ID`) with heartbeats.
- **Model**: OpenAI Responses API via `async-openai` (no hand-rolled client), `store=false`. trex owns all conversation state; output items (including encrypted reasoning and compaction items) are persisted and sent back verbatim. Any provider speaking the Responses API works.
- **Sandbox**: OpenShell gateway over gRPC + mTLS via `openshell-sdk` (git dep pinned to the gateway's version tag). One sandbox per session.

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
provider = "local"
upstream = "..."        # optional, model name sent upstream, defaults to id
```

Env vars:

- `TREX_ADDR` (default `127.0.0.1:8080`)
- `TREX_LOG_FORMAT` = `text` | `json`; `RUST_LOG` overrides filters
- `TREX_CONFIG` (default `trex.toml`)
- `TREX_DATABASE_URL`, `TREX_REDIS_URL` (required; contain credentials, so never log them or put them in `trex.toml`)
- `TREX_OPENSHELL_ENDPOINT` (default `https://127.0.0.1:17670`)
- `TREX_OPENSHELL_TLS_DIR` (default `certs/openshell`, relative to the working dir, containing `ca.crt`, `tls.crt`, `tls.key`; `certs/` is gitignored)

Dev setup: the gateway on `fedora-server` only listens on loopback; tunnel with `ssh -fN -L 17670:127.0.0.1:17670 fedora-server`.

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

### Style

- `rustfmt` defaults, clippy clean. Fix lints rather than `#[allow]`; when an allow is necessary, scope it narrowly and add a one-line reason.
- Comments only when the why isn't obvious from the code; one line.
- Imports grouped: std, external crates, workspace crates, `crate::`.
- Constants for magic values (timeouts, defaults), named for what they mean.
