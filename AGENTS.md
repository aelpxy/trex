# trex

Backend, agent harness and web UI for a Codex/Claude Code-style app on the web. trex owns accounts, sessions (chats), the model catalog, the agent loop and tool execution inside OpenShell sandboxes; the frontend renders and sends input.

`CLAUDE.md` is a symlink to this file.

## Layout

| Path | What it is |
| --- | --- |
| `crates/trex-harness` | The agent: loop (`agent.rs`), history, model catalog (`model::Models`), events, tools (`tool/`), attachments, sandbox files (`files.rs`), lazy sandboxes (`sandbox.rs`) |
| `crates/trex-sandbox` | `OpenShell`, the only wrapper around the OpenShell SDK: workspaces, sandbox lifecycle, exec, TCP forwarding, network access review |
| `crates/trex-store` | Postgres (sqlx, migrations applied on startup), Redis (events, previews), file library (`object_store`) |
| `crates/trex-server` | The `trex` binary: config, axum API (`api/`), runs (`runs.rs`), credits, idle sandboxes, preview proxy (`preview.rs`), the agent's system prompt (`instructions.md`) |
| `crates/trex-eval` | End-to-end eval suite against a server it starts itself |
| `frontend/` | React Router 8 SPA (`ssr: false`), TypeScript, Tailwind, Base UI |
| `images/sandbox/Dockerfile` | The sandbox image |
| `sandbox-policy.yaml` | Default sandbox network policy (OpenShell format) |

Shared dependency versions live in the root `[workspace.dependencies]`; crates opt into features.

## Commands

- `cargo run` runs the server (binary `trex`, from the workspace root; it's the only default member, so other crates need `-p` or `--workspace`)
- `cargo clippy --workspace --all-targets` must be warning-free; `cargo fmt` before committing
- `cargo test --workspace`; `cargo test --workspace -- --ignored` for live tests (OpenShell gateway, the Responses API from `trex.toml`, Postgres/Redis from `.env`)
- `cargo run -p trex-eval -- [--repeat N] [--concurrency N] [--effort LEVEL] [SCENARIO...]` runs the eval suite and writes `target/eval/last-run.json`. Run it before and after agent or prompt changes. It starts its own trex (random port, `target/eval/trex.toml` = `trex.toml` plus `eval-small-context` for compaction and a 15s idle timeout) and uses fixed eval accounts; scenarios live in `scenarios.rs` (`: exclusive` ones restart the server and run last)
- The `trex` binary serves the web app too (`web.rs`): `pnpm --dir frontend build`, then `cargo build --release` embeds `frontend/build/client` with `rust-embed` (debug builds read it from disk, so a rebuilt frontend shows without recompiling). Every path that isn't the api (`/v1/...`, `/health`, `/docs`, `/openapi.json`) gets the app's file or `index.html`; `/assets/*` is cached as immutable, `index.html` never. Unknown `/v1/...` paths get the api's JSON 404. A build without the frontend still compiles and only serves the api.
- Frontend development (`frontend/`, pnpm only): `pnpm dev` serves it with hot reload on :5173 and proxies `/v1` to `TREX_URL` (default `http://127.0.0.1:8080`); `pnpm typecheck`
- CI (`.github/workflows/ci.yml`) runs fmt, clippy with `-D warnings`, the unit tests, and the frontend typecheck and build on every push and pull request; live tests and evals stay local
- Sandbox image, built with the gateway's engine on the gateway host: `podman build -t localhost/trex-sandbox:latest images/sandbox`
- Dev gateway: it listens on loopback on `fedora-server`; tunnel with `ssh -fN -L 17670:127.0.0.1:17670 fedora-server`

## Config

Loaded once in `crates/trex-server/src/config.rs` into `Config` from env vars (`.env` first; real environment variables win) and `trex.toml`. Never read env vars or config files elsewhere; library crates take plain values. `.env.example` lists every variable.

- `TREX_ADDR` (default `127.0.0.1:8080`), `TREX_LOG_FORMAT` = `text` | `json` (`RUST_LOG` overrides filters), `TREX_CONFIG` (default `trex.toml`)
- `TREX_DATABASE_URL`, `TREX_REDIS_URL`: required; they contain credentials, so never log them
- `TREX_OPENSHELL_ENDPOINT` (default `https://127.0.0.1:17670`), `TREX_OPENSHELL_TLS_DIR` (default `certs/openshell` with `ca.crt`, `tls.crt`, `tls.key`; `certs/` is gitignored)
- `TREX_SANDBOX_IMAGE` (default `localhost/trex-sandbox:latest`), `TREX_SANDBOX_POLICY` (default `sandbox-policy.yaml`), `TREX_SANDBOX_IDLE_SECS` (default `300`)
- `TREX_LIBRARY_DIR` (default `data/library`) or `TREX_S3_BUCKET` + `TREX_S3_ENDPOINT`/`_REGION`/`_ACCESS_KEY_ID`/`_SECRET_ACCESS_KEY`/`_FORCE_PATH_STYLE` for any S3-compatible provider
- `TREX_INSECURE_COOKIES` (default `false`; `true` drops `Secure` from the session cookie for plain-http access other than localhost), `TREX_TRUST_PROXY_HEADERS` (default `false`; `true` reads the client IP from `X-Forwarded-For`, only behind a proxy you control; the Vite dev proxy sends it)
- `TREX_PREVIEW_ADDR` (default `127.0.0.1:8081`), `TREX_PREVIEW_URL` (default `http://{id}.preview.localhost:8081`; `{id}` must start the host)

`trex.toml` is the operator's model catalog (`Models::from_toml`). It holds provider api keys, so it's gitignored; `trex.toml.example` is the template:

```toml
[providers.local]
base_url = "http://127.0.0.1:8699/v1"
api_key = "..."

[[models]]
id = "gpt-6.1-sol"      # what the UI selects
name = "GPT 6.1 Sol"    # optional display name, defaults to id
provider = "local"
upstream = "..."        # optional model name sent upstream, defaults to id
context_window = 400000 # optional, defaults to 128000; compaction starts at 80%
reasoning_efforts = ["low", "medium", "high"] # optional levels sessions may pick; any when omitted
fast = true             # optional, allows the priority service tier
price = { input = 1_250_000, cached_input = 125_000, output = 10_000_000 } # optional credits per million tokens; fast_multiplier defaults to 2

[plans.free]            # optional; plans top balances up monthly, without them only admins add funds
name = "Free"
monthly_credits = 5_000_000 # $5; the balance is topped up to this once a month
```

## Tenancy and accounts

- The workspace is the tenant. Sessions, projects, the library, sandboxes, usage and credits belong to a workspace; users are members (`workspace_members`, one personal workspace each for now). Every store function on tenant data takes the workspace id and filters by it.
- Users have a `role`, `user` or `admin`; admins use `/v1/admin` and the Admin page. Grant or revoke it with `trex admin grant|revoke <email>` (runs against the configured database and exits).
- Auth is email + password (argon2id, hashed on a blocking thread). Signing up or in starts a sign-in session (`user_sessions`: sha256 of a random token, 30 days, with the IP and user agent it started from and the last IP it was used from) and sets it as a `session` cookie: `HttpOnly; SameSite=Lax; Path=/; Secure` (`TREX_INSECURE_COOKIES=true` drops `Secure` for plain-http setups; the eval uses it). There are no bearer tokens. Requests that change data must also send an `X-Requested-With` header (`auth::require_csrf_header`), which other sites can't add without a CORS preflight trex never allows. The client IP is the peer address, or the first `X-Forwarded-For` entry only with `TREX_TRUST_PROXY_HEADERS=true`. `api::auth::Account` is the signed-in user and session; `api::auth::Auth` resolves their workspace (the `Trex-Workspace: ws_...` header, or their first workspace) and every workspace handler uses `Auth.workspace` as the tenant. Users list and end their own sessions (`/v1/me/sessions`); admins list and end anyone's (`/v1/admin/users/{id}/sessions`). Email verification and password reset come later (`users.email_verified_at` exists for it).
- OpenShell: one OpenShell workspace per trex workspace (`workspace_name(uuid)` = `u-` + 17 hex chars of sha256, labelled `trex-workspace=<uuid>`, or the legacy `trex-user=<uuid>`, verified on every `ensure_workspace`). trex's mTLS identity is a gateway platform admin, so trex enforces tenancy: never build a `Sandbox { workspace, name }` for a request from anything but its own workspace.

## Agent

- **Model**: the OpenAI Responses API via `async-openai`, `store=false`, any provider speaking it. trex owns all state; output items (including encrypted reasoning) are persisted and sent back verbatim. Every request carries the session id as `prompt_cache_key`. Reasoning is only readable by the model that wrote it: when a run's model differs from the last one, earlier reasoning items are left out of requests (`Store::start_reasoning`, `Agent::reasoning_from`).
- **System prompt**: `crates/trex-server/src/instructions.md`, today's date appended per run, project instructions added. It's model-facing: never name the agent trex in it or anything else the model reads; trex is the harness, not the assistant.
- **Retries**: model requests retry with backoff (5 attempts) on dropped or stalled streams (300s idle), 408/429/5xx and `server_error`/`rate_limit_exceeded`. Tools only run after a response completes, so retries have no side effects; `model.retrying` tells the UI to discard the partial turn.
- **Compaction** (trex's own, works with any provider): at 80% of `context_window`, or on `context_length_exceeded`, the model writes a handoff summary (same prefix with `tool_choice: none` to hit the cache; a trimmed transcript if even that overflows), appended as a developer-role checkpoint (`history::checkpoint`). Requests send items from the latest checkpoint (`history::context_start`); the items API shows it as `compaction`.
- **History** is append-only and saved item by item (`agent::Journal`, keyed by position so repeated saves are no-ops). Dangling tool calls are closed by appending outputs, never inserting.
- **Runs** (`runs.rs`): one per session, claimed with a conditional update. Each holds a lease (`sessions.run_id` + `run_heartbeat_at`, renewed every 10s); every instance resumes runs whose heartbeat is over 30s old (`run.resumed`). A run that loses its lease stops without touching the session; a resumed run whose history already ends with a reply just finishes.
- **Steering**: messages sent mid-run are queued in `sessions.queued_messages`; the agent takes them before every step and when it would finish (`agent::Inbox`). A run only finishes while the queue is empty, so a message sent as it ends continues it; cancelled and failed runs save leftover messages. An interrupt (`agent::Steering::interrupts`, in-memory per instance) drops the current step and closes unfinished tool calls as interrupted.
- **Plans**: `update_plan` keeps a checklist (`plan.updated`); finishing with unfinished steps appends a `[plan reminder]` developer message once per plan. Developer messages are hidden from the items API except checkpoints.
- **Credits** (`credits.rs`) are US dollars in millionths: 1 credit = $0.000001, so `1_000_000` is $1. Each response is charged `ceil((uncached × input + cached × cached_input + output × output) × fast_multiplier? / 1M)` from the model's `price`, recorded with its ledger entry in one transaction (`Store::charge_usage`). `credits::require` refills the monthly allowance when plans are configured, and always refuses new messages at $0 (402 `insufficient_credits_error`), so new users need a plan or funds from an admin before they can chat; a running agent checks `agent::Budget` before each request after its first and stops with `run.failed` `code: insufficient_credits`. Title generation isn't charged.

## Scheduled tasks

- A task (`scheduled_tasks`) is a prompt, model, effort, optional project, and a five-field cron expression read in an IANA timezone (`schedule.rs`, `croner`; at most hourly, checked over the next 24 runs; 20 per workspace).
- `scheduler.rs` ticks every 30s on every instance: `Store::claim_due_tasks` locks due rows (`FOR UPDATE SKIP LOCKED`) and moves each to its next run in the same transaction, so runs are never doubled and a missed slot (server down) runs once, not once per slot.
- Each run is a new chat (`sessions.scheduled_task_id`), titled after the task and the time, started with a hidden developer note quoting the previous run's reply. The model and balance are checked before the chat is created, so a run that can't start leaves no chat; why it didn't start goes to `last_error`. Runs are `unattended`: the agent gets no `ask_user`.
- Task chats aren't in the normal chat list; `GET /v1/sessions?scheduled_task_id=` lists a task's runs.

## Tools

`bash` (with `background`), `read_file`, `write_file`, `edit_file`, `apply_patch`, `grep`, `glob`, `web_fetch`, `library_list`/`library_load`/`library_save`, `view_image`, `process_output`, `stop_process`, `update_plan`, `get_current_time`, `show_preview`, plus `ask_user`.

- `apply_patch` takes Codex's patch format, plans every hunk before writing (all or nothing) and matches context exactly, then ignoring trailing, then surrounding whitespace. Every file tool emits `file.changed` with a unified diff (`similar`, capped at 64 KiB).
- Background processes run under a `setsid` wrapper with output, exit code and control files in `/tmp/.processes/<id>/`, so they survive trex restarts. OpenShell sandboxes each exec, so one can't signal another: `stop_process` drops a `stop` file and the wrapper kills its own process group. Processes die when the sandbox idles out.
- Attachments (`attachment.rs`): images (PNG/JPEG/GIF/WebP), PDFs and text, typed by their bytes. Stored content-addressed at `workspaces/{uuid}/attachments/{sha256}`; history refers to them as `attachment://{sha256}#{mime}` and `attachment::resolve` inlines data urls right before each request. They're also copied into the sandbox at `/sandbox/uploads/<name>` when it's provided (missing files only), and the message carries a hidden note naming those paths (`attachment::uploads_note`; `history::message_text` and the items API leave it out). Tools can return content parts (`Tool::call_content`), which is how `view_image` shows the model an image.
- `show_preview` checks the port answers over `[::1]` and emits `preview.opened`.

## Sandboxes

- One per session, created lazily on the first tool that needs it (`sandbox::LazySandbox`). Exec uses the streaming RPC: stdin is chunked under the 1 MiB gRPC limit and dropping the stream kills the process.
- Idle sandboxes (`idle.rs`): every minute, sessions not running whose `updated_at` is older than the idle timeout get their sandbox stopped (files kept) and `sandbox_stopped` set, with the row locked (`FOR UPDATE SKIP LOCKED`) so a run can't start mid-stop. Starting always checks the real phase.
- A sandbox the gateway marks Error (it can then be neither stopped nor started) or that's gone is replaced with an empty one, announced with `sandbox.replaced` and a note on the tool output so the model knows its files are gone.
- Image: Ubuntu 24.04 with Python/uv, Node (pnpm, TypeScript, tsx, Vite, Tailwind CLI, Biome, prettier, eslint, oxlint, oxfmt), Go, Rust, micromamba and build tools. OpenShell forces `no_new_privs` and a non-root user, so apt never works; the agent installs into `HOME=/sandbox` with pip/uv, npm, go, cargo or micromamba. TLS is intercepted with a per-sandbox CA in `SSL_CERT_FILE`; tools that ignore it need a wrapper (see micromamba).
- Network policy: unlisted egress is denied and becomes a pending access request (`pending_access`, polled during runs, emitted as `access.requested`) that the user approves or rejects. Policy gotchas: an empty `binaries` list matches nothing (use `path: "/**"`), L7 endpoints are audit-only unless `enforcement: enforce`, and the proxy rejects encoded slashes (`%2F`) unless the endpoint is `protocol: rest` with `allow_encoded_slash: true`, which npm needs for scoped packages. Sandboxes keep the policy they were created with.
- IPv4 loopback inside a sandbox goes through OpenShell's proxy and resets: servers must listen on `::` or `localhost` and be reached at `localhost`/`[::1]`.
- Previews (`preview.rs`): a second listener serves `http://{id}.preview.localhost:8081`, the id being the host's first label so every preview is its own origin; ids live in Redis for a day (`trex_store::previews`). Each request gets its own `ForwardTcp` tunnel (`OpenShell::forward`) with a fresh session token, since the gateway allows 3 connections per token and only ends a tunnel once both sides close; hyper reads one response and drops it. Websockets are upgraded and piped. The `Host` header is rewritten to `localhost:<port>` for dev servers.
- Files API (`trex_harness::files`): paths are relative to `/sandbox`; listings skip dependency and cache folders (`node_modules`, `.git`, ...); reads are capped at 10 MB. Listing or reading starts a stopped sandbox but never creates one; writing does.
- Host gotcha: if sandboxes turn Error about 110s after creation with `HealthCheckFailed`, podman's pause process and `podman.service` ended up in different user namespaces; stop the containers through the service (`podman --remote stop`) and restart `podman.service` and `openshell-gateway`.

## HTTP API (v1)

Style: plural resources, prefixed ids (`sess_`, `ws_`, `proj_`, ...), an `object` field on every resource, one error shape (`{"error": {"type", "message", "param"}}`) with `x-request-id` on every response, cursor lists (`limit`, `starting_after`; `{"object": "list", "data", "has_more"}`), SSE events whose `event:` equals the payload `type`, with `Last-Event-ID` resume. Planned, not built: `Idempotency-Key`, a `Trex-Version` header, `metadata` maps, `ending_before`.

Docs: `GET /docs` (Scalar) renders `GET /openapi.json`, generated by utoipa. Every handler has a `#[utoipa::path]` (summary, `operation_id`, tag, params, `ErrorResponse` responses) and is registered with `routes!` in `api::routes()`; `{*path}` wildcard routes are registered by hand and their docs merged in. `spec_documents_every_route` lists every path. Bodies are typed structs deriving `ToSchema`, never `json!`.

- Auth and account: `POST /v1/auth/signup|login|logout` (set or clear the session cookie), `GET|PATCH /v1/me`, `POST /v1/me/password` (signs out the other sessions), `GET /v1/me/sessions`, `DELETE /v1/me/sessions/{id}`, `POST /v1/me/sessions/sign_out_others`
- `GET /v1/models`: `{id, name, context_window, reasoning_efforts, fast}` in `trex.toml` order, limited to the workspace's `allowed_models` when an admin set them; creating, switching or running a chat on another model is refused (403)
- Sessions: `POST|GET /v1/sessions` (`?project_id=none` lists chats outside projects), `GET|PATCH|DELETE /v1/sessions/{id}`. `PATCH {title?, project_id?, model?, reasoning_effort?, fast?}`; model changes apply from the next run. Titles are generated after the first message (`session.updated`).
- Scheduled tasks: `POST|GET /v1/scheduled_tasks`, `GET|PATCH|DELETE /v1/scheduled_tasks/{id}` (`PATCH` takes any field; unpausing or a new schedule picks the next run from now), `POST /v1/scheduled_tasks/{id}/run` (a run now, in a new chat; returns the chat)
- Projects: `POST|GET /v1/projects`, `GET|PATCH|DELETE /v1/projects/{id}`; optional `instructions` apply to every chat in them; deleting keeps the chats
- Sessions carry `queued_messages` (messages sent during the run that the agent hasn't read, oldest first), so a reloaded page shows them
- `GET /v1/sessions/{id}/items` (`message`, `tool_call`, `tool_result`, `reasoning`, `compaction`, each with `seq` and `created_at`), `GET /v1/sessions/{id}/usage` (per response, with `duration_ms` and `first_token_ms`)
- `POST /v1/sessions/{id}/messages` `{content, interrupt?, attachments?: [{data | library_path, filename?}]}`: starts a run (202), or queues the message during one (`queued: true`); content may be empty when there are attachments
- `POST /v1/sessions/{id}/answers` resumes `needs_input`; `POST .../cancel`; `POST .../retry` continues a failed or stopped run from saved history (409 if running, waiting for answers, or already replied)
- `POST /v1/sessions/{id}/branch` `{message, content?}`: a new chat with the history before the user's `message`-th message (from 0), then that message edited (`content`, attachments kept) or as it was, with a run started (201). The original is untouched; the branch gets its own sandbox.
- `GET /v1/sessions/{id}/access_requests`, `POST .../access_requests/{request_id}/approve|reject`
- `GET /v1/sessions/{id}/events`: SSE; `?from=start` replays retained events, `?from=run` the latest run, otherwise the live tail
- `POST /v1/sessions/{id}/previews` `{port}` returns a preview `url`
- `GET /v1/sessions/{id}/files`, `GET|PUT|DELETE /v1/sessions/{id}/files/{path}`, `POST .../files/move`
- Library: `GET /v1/library?prefix`, `GET|PUT|DELETE /v1/library/files/{path}`, `POST /v1/library/move` (never overwrites); `GET /v1/attachments/{id}`
- Credits: `GET /v1/plans`, `GET /v1/credits`, `GET /v1/credits/ledger` (cursor with `starting_after`, or numbered with `page`; returns `total_count`); admin (users with `role: admin`): `GET /v1/admin/overview|usage|logs|models`, `GET /v1/admin/users` and `GET /v1/admin/workspaces` (numbered `page`, `limit`, search `q`; return `total_count`), `PATCH /v1/admin/users/{id}` `{role}`, `POST /v1/admin/users/{id}/sign_out`, `GET /v1/admin/users/{id}/sessions`, `DELETE /v1/admin/users/{id}/sessions/{session_id}`, `PATCH /v1/admin/workspaces/{id}` `{allowed_models}`, `GET /v1/admin/workspaces/{id}/library`, `GET /v1/admin/workspaces/{id}/library/files/{path}`, `POST /v1/admin/workspaces/{id}/credits|plan`

Events: `run.started`, `run.resumed`, `sandbox.creating`, `sandbox.starting`, `sandbox.ready`, `sandbox.replaced`, `text.delta`, `reasoning.delta`, `tool.call.started`, `tool.call.delta`, `tool.call`, `tool.output`, `tool.result`, `file.changed`, `plan.updated`, `preview.opened`, `usage`, `access.requested`, `question`, `model.retrying`, `context.compacting`, `context.compacted`, `message.received`, `run.interrupted`, `session.updated`, then one of `run.completed`, `run.needs_input`, `run.cancelled`, `run.failed`.

## Frontend

- `frontend/app`: `routes/` (layout with auth guard, chat, project, library, auth, account, scheduled, and `admin/` as nested routes per tab), `components/` (the owner's UI components; admin and account ones in their folders), `lib/` (`api.ts` fetch client and SSE reader, `trex.ts` API types and calls, `queries.ts` TanStack Query definitions, `query-client.ts`).
- Server state goes through TanStack Query: every read is a `queryOptions()` in `lib/queries.ts`; route `clientLoader`s prefetch with `queryClient.ensureQueryData` and components read with `useSuspenseQuery`; writes are `useMutation`s that invalidate the affected keys. URL state (filters, tabs, ranges, page numbers) lives in search params; `components/ui/pagination.tsx` is the shared page bar. The chat's live event stream is the exception: it's SSE, handled in `use-chat.ts`.
- The session is an HttpOnly cookie the browser sends itself; `lib/api.ts` adds `X-Requested-With` and `Trex-Workspace` (from the saved UI state). SSE is read with fetch and reconnects with `Last-Event-ID`.
- `components/chat/use-chat.ts` maps events into message parts (`events.ts` is the reducer); reloading mid-run replays it with `?from=run`.
- Previews of agent output run in sandboxed iframes: HTML and React files (`lib/react-preview.ts`, React and npm imports from esm.sh, Tailwind from its browser build) at an opaque origin, live servers at their preview origin.
- Don't restyle or restructure the owner's components beyond wiring; new components follow the existing styles (`components/ui/styles.ts`). Verify with `pnpm typecheck`; the owner tests in the browser.

## Rust practices

- **Structure**: one crate per layer, one module per concern. Use official or well-maintained SDKs, never hand-rolled clients. Keep OpenShell proto types inside `trex-sandbox`; `async-openai` types stay inside the harness and never appear in the HTTP API. Add a trait only for a second implementation or a real test seam. Handlers stay thin: parse, call a domain function, map the result.
- **Errors**: `anyhow::Result` with lowercase `.context("failed to ...")`; a typed error enum where callers branch on it (e.g. HTTP status). No `unwrap`/`expect` outside tests and truly impossible states. Never swallow errors: propagate, or log with `tracing::warn!/error!` and say why continuing is safe.
- **Async**: never block tokio (`spawn_blocking` for blocking work); long-running tasks are cancellable and external calls have timeouts; no locks held across `.await`; channels over shared mutable state.
- **Types**: enums over strings and bools for modes; no `Debug` derives on structs holding secrets; serde types for external data tolerate unknown fields (keep raw `serde_json::Value` for items we don't model); borrow in parameters.
- **Logging**: `tracing` with structured fields and short lowercase messages; never log secrets, tokens or full prompts at `info` or above.
- **Dependencies**: `cargo add` with only the needed features; prefer std or an existing dep; one version of tonic/hyper/rustls (`cargo tree -d`; known duplicate: reqwest 0.12 from openshell-sdk and 0.13 from async-openai); git deps pinned to a tag or rev.
- **Testing**: unit tests next to the code; tests needing external services are `#[ignore]` with a comment saying what they need; assert on concrete values.
- **SQL**: UPPERCASE keywords and types, lowercase identifiers, no comments in SQL. Migrations in `crates/trex-store/migrations` are never edited once committed. Every query on tenant data filters by `workspace_id`.
- **Style**: rustfmt defaults, clippy clean (fix lints; a necessary `#[allow]` is scoped and explained in one line). Comments only for a non-obvious why, one line. Imports grouped std, external, workspace, `crate::`. Named constants for magic values.
