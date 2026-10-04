<p align="center">
  <img src="assets/trex-logo-universal.png" alt="Trex" width="480">
</p>

# Trex

Fast agentic hardening and sandboxing engine written in Rust.

Trex combines an agent harness with OpenShell sandboxes to run tools in isolated workspaces, stream execution output, and surface denied network access for review.

## Status

Trex is under active development. The workspace includes the agent harness, sandbox integration, and storage layer. The HTTP server currently exposes `GET /health`; agent execution is available through the Rust harness and is not yet exposed through HTTP.

## Components

| Crate | Purpose |
| --- | --- |
| `trex-harness` | Streaming agent loop, tool execution, model configuration, and user questions. |
| `trex-sandbox` | OpenShell connectivity over mutual TLS, per-user workspaces, sandbox lifecycle, command streaming, and network access review. |
| `trex-store` | PostgreSQL and Redis connections, with library storage support. |
| `trex-server` | The `trex` HTTP server, health checks, request tracing, and graceful shutdown. |

The repository also includes a development sandbox image in [`images/sandbox/Dockerfile`](images/sandbox/Dockerfile) and a network policy in [`sandbox-policy.yaml`](sandbox-policy.yaml).

## Getting started

### Requirements

- A Rust toolchain supporting edition 2024.
- A Unix environment for the server's signal handling.
- PostgreSQL and Redis.
- A running OpenShell gateway compatible with the pinned `v0.1.2` SDK, with client TLS certificates.
- A model provider supporting the OpenAI Responses API to run agents.

### Configuration

Copy the model configuration and edit it for your provider:

```sh
cp trex.toml.example trex.toml
```

Configure provider URLs, API keys, and model IDs in `trex.toml`. This file is ignored by Git.

Create a `.env` file with your database and Redis connection URLs:

```dotenv
TREX_DATABASE_URL=postgres://postgres:postgres@127.0.0.1:5432/trex
TREX_REDIS_URL=redis://127.0.0.1:6379
```

Place the gateway's `ca.crt`, `tls.crt`, and `tls.key` files in `certs/openshell/`, or set `TREX_OPENSHELL_TLS_DIR` to their directory.

| Variable | Default | Purpose |
| --- | --- | --- |
| `TREX_DATABASE_URL` | Required | PostgreSQL connection URL. |
| `TREX_REDIS_URL` | Required | Redis connection URL. |
| `TREX_ADDR` | `127.0.0.1:8080` | HTTP listen address. |
| `TREX_OPENSHELL_ENDPOINT` | `https://127.0.0.1:17670` | OpenShell gateway endpoint. |
| `TREX_OPENSHELL_TLS_DIR` | `certs/openshell` | Gateway client certificate directory. |
| `TREX_CONFIG` | `trex.toml` | Model configuration path. |
| `TREX_LOG_FORMAT` | `text` | Log output: `text` or `json`. |
| `RUST_LOG` | `info`, with debug logging for Trex | Tracing filter. |

Environment variables take precedence over `.env`. The server runs PostgreSQL migrations on startup.

### Run

```sh
cargo run -p trex-server --bin trex
```

With the required services available, check the server:

```sh
curl http://127.0.0.1:8080/health
```

The endpoint returns the package version and `status: "ok"` when PostgreSQL and Redis respond. It returns HTTP 503 with `status: "unavailable"` when either check fails.

## Development

```sh
cargo check --workspace
cargo test --workspace
```

Live integration tests are ignored by default. Run them with `cargo test --workspace -- --ignored` after configuring their services. Sandbox tests require the gateway at `127.0.0.1:17670` and certificates in `certs/openshell`; development-image tests also require `localhost/trex-sandbox:latest` to be built on the gateway host. Storage tests require the database and Redis environment variables.

## License

Licensed under the [Apache License, Version 2.0](LICENSE).
