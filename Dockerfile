FROM docker.io/library/node:24-slim AS web
WORKDIR /src/frontend
RUN corepack enable
COPY frontend/package.json frontend/pnpm-lock.yaml ./
RUN pnpm install --frozen-lockfile
COPY frontend/ ./
RUN pnpm build

FROM docker.io/library/rust:1-slim-trixie AS server

RUN apt-get update \
    && apt-get install -y --no-install-recommends build-essential cmake \
    && rm -rf /var/lib/apt/lists/*
WORKDIR /src
COPY . .
COPY --from=web /src/frontend/build/client frontend/build/client

RUN --mount=type=cache,target=/usr/local/cargo/registry \
    --mount=type=cache,target=/usr/local/cargo/git \
    --mount=type=cache,target=/src/target \
    cargo build --release --locked -p trex-server \
    && cp target/release/trex /usr/local/bin/trex

FROM docker.io/library/debian:trixie-slim
RUN apt-get update \
    && apt-get install -y --no-install-recommends ca-certificates curl \
    && rm -rf /var/lib/apt/lists/*
COPY --from=server /usr/local/bin/trex /usr/local/bin/trex
COPY sandbox-policy.yaml /etc/trex/sandbox-policy.yaml
ENV TREX_CONFIG=/etc/trex/trex.toml \
    TREX_SANDBOX_POLICY=/etc/trex/sandbox-policy.yaml \
    TREX_OPENSHELL_TLS_DIR=/etc/trex/openshell \
    TREX_LIBRARY_DIR=/var/lib/trex/library \
    TREX_LOG_FORMAT=json \
    TREX_ADDR=0.0.0.0:8080 \
    TREX_PREVIEW_ADDR=0.0.0.0:8081
WORKDIR /var/lib/trex

ENTRYPOINT ["trex"]
