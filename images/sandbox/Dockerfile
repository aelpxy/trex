# dev image for trex session sandboxes
# tools live under /usr because the sandbox policy only grants read access to system paths.
# openshell forces no_new_privs and a non-root user, so apt cannot run inside a sandbox;
# the agent installs extra software with pip/uv, npm, go, cargo, or micromamba (conda-forge) instead.
FROM docker.io/library/ubuntu:24.04

ARG NODE_MAJOR=24
ARG TARGETARCH

RUN apt-get update \
    && apt-get install -y --no-install-recommends \
        bash ca-certificates curl wget git git-lfs openssh-client gnupg \
        build-essential cmake ninja-build pkg-config libssl-dev \
        python3 python3-dev python3-pip python3-venv python-is-python3 \
        iproute2 nftables \
        jq ripgrep fd-find sqlite3 unzip zip xz-utils bzip2 file less tree procps \
        rsync patch bc netcat-openbsd \
    && ln -s /usr/bin/fdfind /usr/local/bin/fd \
    && rm -rf /var/lib/apt/lists/*

RUN case "${TARGETARCH:-$(dpkg --print-architecture)}" in \
        amd64) echo "node=x64 go=amd64 mamba=linux-64" ;; \
        arm64) echo "node=arm64 go=arm64 mamba=linux-aarch64" ;; \
        *) exit 1 ;; \
    esac > /tmp/arch

RUN . /tmp/arch \
    && base="https://nodejs.org/dist/latest-v${NODE_MAJOR}.x" \
    && file="$(curl -fsSL "$base/SHASUMS256.txt" | grep -o "node-v[0-9.]*-linux-${node}.tar.xz" | head -n1)" \
    && curl -fsSLO "$base/$file" \
    && curl -fsSL "$base/SHASUMS256.txt" | grep " $file\$" | sha256sum -c - \
    && tar -xJf "$file" -C /usr/local --strip-components=1 --exclude CHANGELOG.md --exclude LICENSE --exclude README.md \
    && rm "$file"

RUN . /tmp/arch \
    && version="$(curl -fsSL 'https://go.dev/VERSION?m=text' | head -n1)" \
    && curl -fsSL "https://go.dev/dl/${version}.linux-${go}.tar.gz" | tar -xz -C /usr/local

ENV RUSTUP_HOME=/usr/local/rustup
RUN curl -fsSL https://sh.rustup.rs | env CARGO_HOME=/usr/local/cargo sh -s -- -y --no-modify-path \
        --profile minimal --default-toolchain stable --component clippy,rustfmt \
    && chmod -R a+rX /usr/local/rustup /usr/local/cargo

RUN curl -LsSf https://astral.sh/uv/install.sh | env UV_INSTALL_DIR=/usr/local/bin UV_NO_MODIFY_PATH=1 sh

# openshell exports its per-sandbox tls ca via SSL_CERT_FILE at a driver-specific path, which micromamba
# ignores, so a wrapper forwards it at runtime
RUN . /tmp/arch \
    && install -d /usr/local/libexec \
    && curl -fsSL "https://micro.mamba.pm/api/micromamba/${mamba}/latest" | tar -xj -C /tmp bin/micromamba \
    && install -m 0755 /tmp/bin/micromamba /usr/local/libexec/micromamba \
    && printf '%s\n' '#!/bin/sh' 'exec env MAMBA_SSL_VERIFY="${MAMBA_SSL_VERIFY:-${SSL_CERT_FILE:-true}}" /usr/local/libexec/micromamba "$@"' \
        > /usr/local/bin/micromamba \
    && chmod 0755 /usr/local/bin/micromamba \
    && rm -rf /tmp/bin /tmp/arch

RUN groupadd --gid 1500 agent \
    && useradd --uid 1500 --gid agent --home-dir /sandbox --shell /bin/bash agent \
    && install -d -o agent -g agent /sandbox

# home is the writable workdir, so per-user installs land somewhere the read-only policy allows
ENV HOME=/sandbox \
    XDG_CACHE_HOME=/tmp/.cache \
    PIP_BREAK_SYSTEM_PACKAGES=1 \
    PIP_USER=1 \
    PYTHONUSERBASE=/sandbox/.local \
    NPM_CONFIG_PREFIX=/sandbox/.local \
    GOPATH=/sandbox/go \
    GOTOOLCHAIN=local \
    CARGO_HOME=/sandbox/.cargo \
    MAMBA_ROOT_PREFIX=/sandbox/.mamba \
    PATH=/sandbox/.local/bin:/sandbox/go/bin:/sandbox/.cargo/bin:/sandbox/.mamba/bin:/usr/local/go/bin:/usr/local/cargo/bin:/usr/local/bin:/usr/bin:/bin

WORKDIR /sandbox
USER agent
