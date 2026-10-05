<p align="center">
  <img src="assets/trex-logo-universal.png" alt="Trex" width="480">
  <br>
  <strong>Trex</strong>
  <br>
  An agentic execution engine built in Rust, powered by isolated sandboxes.
</p>

Trex brings agents to life with streaming responses, isolated tool execution, and persistent conversations and files. Network access stays under your control.

## Status

Trex is under active development.

## Components

| Crate          | Purpose                                                                                                                   |
| -------------- | ------------------------------------------------------------------------------------------------------------------------- |
| `trex-harness` | Streaming agent loop, tool execution, model configuration, and user questions.                                            |
| `trex-sandbox` | OpenShell connectivity over mutual TLS, sandbox lifecycle, command streaming, port forwarding, and network access review. |
| `trex-store`   | PostgreSQL persistence, Redis events and coordination, and local or S3-compatible file storage.                           |
| `trex-server`  | The `trex` HTTP API, authentication, agent runs, credits, preview proxy, and sandbox idle management.                     |

| `trex-eval` | Live end-to-end scenarios against a managed server instance. |
| `frontend` | React, React Router, TypeScript, and Tailwind web interface. |

The repository also includes a development sandbox image in [`images/sandbox/Dockerfile`](images/sandbox/Dockerfile) and a network policy in [`sandbox-policy.yaml`](sandbox-policy.yaml).

## Getting started

### Requirements

- A Rust toolchain supporting edition 2024.
- A Unix environment for the server's signal handling.
- PostgreSQL and Redis.
- A running OpenShell gateway compatible with the pinned `v0.1.2` SDK, with client TLS certificates.
- A model provider supporting the OpenAI Responses API to run agents.
- Node.js compatible with the frontend dependencies and pnpm for the web interface.
- The sandbox image built on the OpenShell gateway host.

### Configuration

Copy the example configuration files and edit them for your services:

```sh
cp trex.toml.example trex.toml
cp .env.example .env
```

Configure provider URLs, API keys, and model IDs in `trex.toml`. This file is ignored by Git.

Set your database and Redis connection URLs in `.env`, for example:

```dotenv
TREX_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/trex
TREX_REDIS_URL=redis://127.0.0.1:6379
```

Place the gateway's `ca.crt`, `tls.crt`, and `tls.key` files in `certs/openshell/`, or set `TREX_OPENSHELL_TLS_DIR` to their directory.

| Variable                  | Default                              | Purpose                                                        |
| ------------------------- | ------------------------------------ | -------------------------------------------------------------- |
| `TREX_DATABASE_URL`       | Required                             | PostgreSQL connection URL.                                     |
| `TREX_REDIS_URL`          | Required                             | Redis connection URL.                                          |
| `TREX_ADDR`               | `127.0.0.1:8080`                     | HTTP listen address.                                           |
| `TREX_OPENSHELL_ENDPOINT` | `https://127.0.0.1:17670`            | OpenShell gateway endpoint.                                    |
| `TREX_OPENSHELL_TLS_DIR`  | `certs/openshell`                    | Gateway client certificate directory.                          |
| `TREX_CONFIG`             | `trex.toml`                          | Model catalog and billing plan configuration path.             |
| `TREX_SANDBOX_IMAGE`      | `localhost/trex-sandbox:latest`      | Session sandbox image on the gateway host.                     |
| `TREX_SANDBOX_POLICY`     | `sandbox-policy.yaml`                | Default sandbox network and filesystem policy.                 |
| `TREX_SANDBOX_IDLE_SECS`  | `300`                                | Idle time before stopping a sandbox; files remain for restart. |
| `TREX_LIBRARY_DIR`        | `data/library`                       | Local file library directory when S3 is not configured.        |
| `TREX_PREVIEW_ADDR`       | `127.0.0.1:8081`                     | Separate preview proxy listen address.                         |
| `TREX_PREVIEW_URL`        | `http://{id}.preview.localhost:8081` | Preview URL template; the host must start with `{id}.`.        |
| `TREX_LOG_FORMAT`         | `text`                               | Log output: `text` or `json`.                                  |
| `RUST_LOG`                | `info`, with debug logging for Trex  | Tracing filter.                                                |

Environment variables take precedence over `.env`. The server runs PostgreSQL migrations on startup and connects to the OpenShell gateway before accepting API requests.

To use S3-compatible library storage, set `TREX_S3_BUCKET`, `TREX_S3_ACCESS_KEY_ID`, and `TREX_S3_SECRET_ACCESS_KEY`. Optional settings are `TREX_S3_ENDPOINT`, `TREX_S3_REGION` (default `us-east-1`), and `TREX_S3_FORCE_PATH_STYLE` (default `false`). See [`.env.example`](.env.example).

The model catalog supports provider URLs and keys, upstream model names, context windows, reasoning efforts, fast mode, and token prices. Optional `[plans.*]` entries enable credit enforcement; without plans, credits are tracked but not enforced. See [`trex.toml.example`](trex.toml.example).

### Sandbox image

On the gateway host, build the included image using its container runtime, for example:

```sh
docker build -t localhost/trex-sandbox:latest images/sandbox
```

The image includes Python, Node.js, pnpm, Go, Rust, uv, micromamba, and common development tools.

### Run

```sh
cargo run -p trex-server --bin trex
```

With the required services available, check the server:

```sh
curl http://127.0.0.1:8080/health
```

The endpoint returns the package version and `status: "ok"` when PostgreSQL and Redis respond. It returns HTTP 503 with `status: "unavailable"` when either check fails.

Start the frontend in another terminal:

```sh
cd frontend
pnpm install
pnpm dev
```

Open the URL printed by Vite and sign up or log in. Vite proxies `/api` to `http://127.0.0.1:8080`; set `TREX_URL` when the backend runs elsewhere.

Live previews use a separate origin for each link and expire after 24 hours. Preview apps must listen on `::` or `localhost` with IPv6 support. For remote hosting, configure DNS and a reverse proxy for the preview URL template and forward traffic to `TREX_PREVIEW_ADDR`.

## API

Interactive API documentation is available at `http://127.0.0.1:8080/docs`, with the schema at `/openapi.json`. Routes under `/v1` cover authentication, accounts, models, projects, sessions, messages, event streams, files, previews, library storage, and credits.

Use the token returned by signup or login as `Authorization: Bearer <token>`. Requests select a workspace with the `Trex-Workspace` header, defaulting to the user's first workspace. Session events stream over server-sent events and support replay with `Last-Event-ID`.

The Rust server serves the API and preview proxy. For a production frontend, serve `frontend/build/client` with an SPA fallback and proxy `/api` to the backend with that prefix removed. The frontend also accepts a build-time `VITE_TREX_API` base URL. See [`frontend/README.md`](frontend/README.md).

## Development

```sh
cargo check --workspace
cargo test --workspace
```

Frontend checks, from `frontend/`:

```sh
pnpm typecheck
pnpm build
```

Live integration tests are ignored by default. Run them with `cargo test --workspace -- --ignored` after configuring their services. Sandbox tests require the gateway at `127.0.0.1:17670` and certificates in `certs/openshell`; development-image tests also require `localhost/trex-sandbox:latest` to be built on the gateway host. Storage tests require the database and Redis environment variables.

The live evaluation runner requires the configured services and a `gpt-6.1-sol` model in the catalog:

```sh
cargo run -p trex-eval -- --repeat 1 --concurrency 4
```

It builds and starts its own server, runs scenarios using a dedicated evaluation account, and writes results and logs under `target/eval/`. It clears that account's library and deletes scenario sessions. Pass scenario name filters or `--effort LEVEL` to narrow a run.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
