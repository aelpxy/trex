# trex

Backend, agent harness and web UI for a Codex/Claude Code-style app on the web. trex owns accounts, chats (sessions), the model catalog, the agent loop and tool execution in OpenShell sandboxes; the frontend renders and sends input. `CLAUDE.md` is a symlink to this file.

## Layout

| Path | What it is |
| --- | --- |
| `crates/trex-harness` | The agent: loop (`agent.rs`), history, model catalog (`model::Models`), events, tools (`tool/`), attachments, sandbox files (`files.rs`), lazy sandboxes (`sandbox.rs`) |
| `crates/trex-sandbox` | `OpenShell`, the only wrapper around the OpenShell SDK |
| `crates/trex-store` | Postgres (sqlx, migrations run on startup; `sessions/` split into chats, runs, sandboxes, items), Redis (events, previews), file library (`object_store`) |
| `crates/trex-server` | The `trex` binary: config, axum API (`api/`; `admin/` and `sessions/` are folders of handlers by concern), runs, credits, scheduler, idle sandboxes, previews, user lifecycle (`users.rs`), system prompt (`instructions.md`) |
| `crates/trex-eval` | End-to-end eval suite against a server it starts itself |
| `frontend/` | React Router 8 SPA (`ssr: false`), TypeScript, Tailwind v4, Base UI, TanStack Query and Table |
| `images/sandbox/Dockerfile`, `sandbox-policy.yaml` | Sandbox image and default network policy |

Shared dependency versions live in the root `[workspace.dependencies]`.

## Commands

- `cargo run` (binary `trex`, the only default member; other crates need `-p` or `--workspace`)
- `cargo fmt`; `cargo clippy --workspace --all-targets` warning-free; `cargo test --workspace`
- `cargo test --workspace -- --ignored`: live tests (Postgres/Redis from `.env`, the OpenShell gateway, the Responses API from `trex.toml`). They share the dev database, so they create their own rows and never claim or mutate other data.
- `cargo run -p trex-eval -- [--repeat N] [--concurrency N] [--effort LEVEL] [SCENARIO...]`: run before and after agent or prompt changes; writes `target/eval/last-run.json`
- Frontend (`frontend/`, pnpm only): `pnpm dev` (:5173, proxies `/v1` to `TREX_URL`), `pnpm typecheck`, `pnpm build`
- `trex` serves the built app (`web.rs`): release builds embed `frontend/build/client`, debug builds read it from disk
- CI runs fmt, clippy `-D warnings`, unit tests, frontend typecheck and build
- Sandbox image, on the gateway host: `podman build -t localhost/trex-sandbox:latest images/sandbox` (copy `images/sandbox` over with `scp` and build there; only new sandboxes get a new image); dev gateway tunnel: `ssh -fN -L 17670:127.0.0.1:17670 fedora-server`

## Config

Loaded once in `trex-server/src/config.rs` from env (`.env`, real env wins) and `trex.toml`; nothing else reads env or config files. `.env.example` lists every variable; `TREX_DATABASE_URL` and `TREX_REDIS_URL` hold credentials, never log them. `trex.toml` (gitignored, template `trex.toml.example`) is the model catalog: providers with api keys, `[[models]]` (`id`, `name`, `provider`, `upstream`, `context_window`, `reasoning_efforts`, `fast`, `price` in credits per million tokens) and optional `[plans.*]` with `monthly_credits`.

## Tenancy and accounts

- The workspace is the tenant: chats, projects, library, sandboxes, usage and credits belong to it; users are members (`workspace_members`, one personal workspace each for now). Every store function on tenant data takes the workspace id and filters by it. Admin queries are the only cross-tenant ones.
- Auth: email + password (argon2id on a blocking thread) → `session` cookie (`HttpOnly; SameSite=Lax; Secure` unless `TREX_INSECURE_COOKIES`), 30-day `user_sessions` rows. No bearer tokens. Writes need an `X-Requested-With` header (CSRF). `api::auth::Account` is the user; `api::auth::Auth` adds the workspace (`Trex-Workspace` header or their first).
- Roles `user` | `admin` (`trex admin grant|revoke <email>`). Suspended users (`users.suspended_at`) can't sign in, their sessions stop resolving, and scheduled tasks in workspaces with no unsuspended member wait. Deleting a user (`users.rs`) removes every workspace only they belong to, with its sandboxes, library and events. Admins can't change, suspend or delete themselves, and a change that would leave no active admin is refused (409).
- OpenShell: one OpenShell workspace per trex workspace (`workspace_name(uuid)`, labelled `trex-workspace=<uuid>`). trex's identity is a gateway admin, so trex enforces tenancy: never address a sandbox for a request from another workspace.

## Agent and runs

- Model: the OpenAI Responses API via `async-openai`, `store=false`; trex persists every output item (encrypted reasoning included) and sends them back verbatim; `prompt_cache_key` is the session id. Reasoning from a different model is left out (`Agent::reasoning_from`).
- System prompt `instructions.md` plus date and project instructions. It's model-facing: never name the agent trex there or anywhere the model reads.
- Retries: 5 attempts with backoff on dropped/stalled streams, 408/429/5xx; tools run only after a response completes, so retries are side-effect free (`model.retrying`).
- Compaction: at 80% of `context_window` or on `context_length_exceeded`, the model writes a handoff summary appended as a developer checkpoint; requests start from the latest one. `POST /v1/sessions/{id}/compact` does it on request as a compaction-only run (`Agent::compact_context`); a checkpoint right after a reply still counts as replied, so a resumed or retried run doesn't answer again.
- History is append-only, saved item by item; dangling tool calls are closed by appending outputs.
- Runs (`runs.rs`): one per session via a conditional update, holding a lease (`run_id`, `run_heartbeat_at` renewed every 10s; stale after 30s and resumed by any instance). Cancelling sets `run_cancel_requested`; the holder sees it at its next heartbeat, so cancels work across instances (`runs::cancel`), and a stopping run takes no more queued messages. Suspending or deleting a user cancels this way; deleting waits for their runs to stop before removing sandboxes and files.
- Steering: mid-run messages queue in `sessions.queued_messages` and are read before each step; a run only finishes with an empty queue. Interrupts are in-memory per instance.
- Credits are US dollars in millionths (`1_000_000` = $1), charged per response from the model's `price` with a ledger entry in one transaction. Plans top balances up monthly; new messages are refused at $0 (402), and a running agent stops with `code: insufficient_credits`.
- Scheduled tasks: cron (five fields, IANA timezone, at most hourly, 20 per workspace) claimed with `FOR UPDATE SKIP LOCKED` every 30s; each run is a new hidden chat, `unattended` (no `ask_user`, no `schedule_task`); a missed slot runs once. Creation and validation live in `tasks.rs`, shared by the API and the agent's `schedule_task` tool (`tasks::ChatScheduler`, the harness's `TaskScheduler`), which schedules with the chat's model and project.

## Tools and sandboxes

- Tools: `bash` (with `background`), file tools (`read_file`, `write_file`, `edit_file`, `apply_patch`, `grep`, `glob`), `web_fetch`, `library_*`, `view_image`, `process_output`, `stop_process`, `update_plan`, `get_current_time`, `show_preview`, `browse`, `schedule_task`, `ask_user`. File tools emit `file.changed` with a diff.
- `browse` drives headless Chromium in the sandbox (Playwright from the image; the script `tool/browse.mjs` is sent with each call, so changing it needs no new image) and returns a screenshot, console errors, failed requests and an aria outline. Chromium runs with `--no-sandbox --no-zygote`, since OpenShell's seccomp filter crashes its zygote; local servers are opened at `localhost`. Image results read as `[image: attachment://…]` in tool output, which the chat shows.
- Background processes run under a `setsid` wrapper in `/tmp/.processes/<id>/`; `stop_process` drops a `stop` file since one exec can't signal another.
- Attachments are content-addressed at `workspaces/{uuid}/attachments/{sha256}`, referenced as `attachment://` in history and inlined right before each request; they're also copied to `/sandbox/uploads/`.
- One sandbox per chat, created lazily; idle ones are stopped after `TREX_SANDBOX_IDLE_SECS`; ones in Error or gone are replaced (`sandbox.replaced`).
- OpenShell gotchas: no apt (non-root, `no_new_privs`), install into `HOME=/sandbox`; TLS is intercepted (`SSL_CERT_FILE`); IPv4 loopback resets, so servers listen on `::`/`localhost`; unlisted egress becomes an access request; a `bash` command that was denied waits for the user's decision (`access.rs`, up to 10 min; unattended runs don't wait) and its output tells the model the answer, so it retries on its own; policy: empty `binaries` matches nothing, L7 rules need `enforcement: enforce`, `%2F` needs `allow_encoded_slash`.
- Previews: `{id}.preview.localhost:8081`, one `ForwardTcp` tunnel per request (3 connections per token), websockets piped, `Host` rewritten.
- Sandboxes turning Error ~110s after creation with `HealthCheckFailed`: podman's pause process and `podman.service` are in different user namespaces; stop containers via `podman --remote`, restart `podman.service` and `openshell-gateway`.

## HTTP API (v1)

- The spec is the reference: `GET /docs` renders `/openapi.json` (utoipa). Every handler has `#[utoipa::path]` and is registered with `routes!` in `api::routes()`; `spec_documents_every_route` lists every path, so add new ones there. Bodies are typed `ToSchema` structs, never `json!`.
- Style: plural resources, prefixed ids (`sess_`, `ws_`, `proj_`, `user_`, ...), an `object` field, one error shape `{"error": {"type", "message", "param"}}`, `x-request-id` on every response, cursor lists (`limit`, `starting_after`, `has_more`), numbered pages where a UI pages (`page`, `total_count`), admin lists also take `q` and `sort` (`column` or `-column`).
- SSE: `GET /v1/sessions/{id}/events`, `event:` equals the payload `type`, `Last-Event-ID` resume, `?from=start|run`. Events end with one of `run.completed`, `run.needs_input`, `run.cancelled`, `run.failed`.

## Frontend

- `app/routes/` (one file per page, admin and account tabs nested), `app/components/<area>/`, `app/lib/` (`api.ts` fetch client, `trex.ts` types and calls, `queries.ts`, `toasts.ts`).
- Server state: every read is a `queryOptions()` in `lib/queries.ts`, prefetched in `clientLoader` with `ensureQueryData` and read with `useSuspenseQuery`; writes are `useMutation`s that invalidate the affected keys. The chat stream is SSE (`use-chat.ts`, reducer in `events.ts`).
- URL state (search, folder, tabs, pages, sorts) lives in search params.
- Shared UI in `components/ui/`: `button.tsx` (sizes `lg` beside h-10 inputs, `md` forms, `sm` toolbars and panels, `xs` inside the conversation), `data-table.tsx` (TanStack Table: sorting, row selection, per-row props), `side-drawer.tsx` (glass, like dialogs), `selection-bar.tsx`, `stat.tsx`, `filter-input.tsx`, `toaster.tsx`, `meter.tsx`, `checkbox.tsx`, `switch.tsx`, `select-field.tsx`, `pagination.tsx`, `tab-nav.tsx` (`tabLink` for url segment filters), `empty-state.tsx`, dialogs; `styles.ts` has `fieldLabel`, `badge`, `dangerBadge`, menu and popup styles. Formatting (`count`, `plural`, `tokens`, `date`, `ago`) lives in `lib/format.ts`, url list state in `lib/use-url-filter.ts`. Build on Base UI parts, never native controls, and reuse these before writing new ones.
- Library (`components/library/`): a file manager over the flat library api; folders are path prefixes (`entries.ts`), so folder renames and moves move every file under them. Rows share one action list for the ⋯ menu and right-click, drag onto folders and breadcrumbs to move, and accept desktop files.
- The composer takes slash commands (`chat/slash-commands.ts`, built per chat in `chat-commands.ts`): `/compact`, `/new`, `/model`, `/effort`, `/fast`, `/rename`, `/retry`, `/stop`, offered only when they can run; the context meter beside Send shows the last response's tokens against the model's window.
- Previews of agent output run in sandboxed iframes (`lib/react-preview.ts`).

## UX rules

Check every UI change against these before calling it done:

- Feedback: progress and outcomes of actions go in toasts (`trackToast`, `toasts.add`), never status text dumped on the page; form field errors sit by the field. Destructive actions confirm; reversible ones don't. Don't disable the whole UI while something runs.
- Affordances: menus on ⋯ and right-click, drag and drop where items move, keyboard (Enter, Space, Delete), selection with bulk actions.
- States: empty, loading (Suspense), error, and long or many items handled; nothing overwritten without asking.
- Mobile: touch targets, no hover-only actions (`pointer-coarse:`), 16px inputs.
- Accessibility: labelled controls, `aria-*` from Base UI, focus rings (`styles.ts`).
- Follow existing styles; don't restyle the owner's components beyond what the change needs. Verify with `pnpm typecheck` and `pnpm build`; the owner tests in the browser.

## Code practices

- Keep files small and focused: split a module into a folder of files once it holds several concerns (as `api/admin/`, `api/sessions/`, `trex-store/src/sessions/`, `components/library/`); large test suites go in a sibling `tests.rs`. Handlers and components stay thin: parse, call a domain function or hook, render.
- Use official or maintained SDKs, never hand-rolled clients. OpenShell types stay in `trex-sandbox`, `async-openai` types in the harness. Add a trait only for a second implementation or a real test seam.
- Errors: `anyhow` with lowercase `.context("failed to ...")`; typed enums where callers branch. No `unwrap`/`expect` outside tests. Never swallow errors; log with why continuing is safe.
- Async: never block tokio; cancellable long tasks, timeouts on external calls, no locks across `.await`.
- Types: enums over strings and bools; no `Debug` on structs with secrets; tolerate unknown fields in external data.
- Logging: `tracing`, structured fields, short lowercase messages; never secrets, tokens or full prompts at `info`+.
- Dependencies: `cargo add` with minimal features; one version of tonic/hyper/rustls (known duplicate: reqwest 0.12/0.13).
- Tests next to the code; external-service tests are `#[ignore]` with a comment naming what they need; assert concrete values.
- SQL: UPPERCASE keywords, lowercase identifiers, no comments; build dynamic SQL with `QueryBuilder` (sqlx rejects `format!`); committed migrations are never edited.
- Style: rustfmt, clippy clean (a needed `#[allow]` is scoped with a one-line reason), comments only for a non-obvious why, imports grouped std / external / workspace / `crate::`, named constants for magic values.
