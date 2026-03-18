# syntax=docker/dockerfile:1.7

FROM ubuntu:22.04 AS builder

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

ARG DEBIAN_FRONTEND=noninteractive

ENV CARGO_HOME=/usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup
ENV PATH=/usr/local/cargo/bin:${PATH}

RUN apt-get update && \
    apt-get install -y --no-install-recommends \
      build-essential \
      ca-certificates \
      curl \
      git \
      libssl-dev \
      pkg-config && \
    rm -rf /var/lib/apt/lists/*

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain stable

WORKDIR /workspace

COPY Cargo.toml Cargo.lock ./
COPY src ./src
COPY tests ./tests

RUN cargo build --locked --release \
      --bin computer-mcp \
      --bin computer-mcpd \
      --bin computer-mcp-prd

FROM ubuntu:22.04

SHELL ["/bin/bash", "-o", "pipefail", "-c"]

ARG DEBIAN_FRONTEND=noninteractive
ARG NODE_MAJOR=22
ARG GO_VERSION=1.24.1
ARG RUST_TOOLCHAIN=stable
ARG TARGETARCH

LABEL org.opencontainers.image.source="https://github.com/amxv/computer-mcp"
LABEL org.opencontainers.image.description="Runpod-ready computer-mcp image with Node.js, Python, Go, Rust, git, GitHub CLI, SSH, and common Unix tools preinstalled."
LABEL org.opencontainers.image.licenses="MIT"

ENV CARGO_HOME=/usr/local/cargo
ENV RUSTUP_HOME=/usr/local/rustup
ENV PATH=/usr/local/cargo/bin:/usr/local/go/bin:${PATH}

RUN mkdir -p /etc/apt/keyrings /usr/share/keyrings && \
    curl -fsSL https://deb.nodesource.com/gpgkey/nodesource-repo.gpg.key | gpg --dearmor -o /etc/apt/keyrings/nodesource.gpg && \
    echo "deb [signed-by=/etc/apt/keyrings/nodesource.gpg] https://deb.nodesource.com/node_${NODE_MAJOR}.x nodistro main" > /etc/apt/sources.list.d/nodesource.list && \
    curl -fsSL https://cli.github.com/packages/githubcli-archive-keyring.gpg | dd of=/usr/share/keyrings/githubcli-archive-keyring.gpg && \
    chmod go+r /usr/share/keyrings/githubcli-archive-keyring.gpg && \
    echo "deb [arch=$(dpkg --print-architecture) signed-by=/usr/share/keyrings/githubcli-archive-keyring.gpg] https://cli.github.com/packages stable main" > /etc/apt/sources.list.d/github-cli.list && \
    apt-get update && \
    apt-get install -y --no-install-recommends \
      bash-completion \
      build-essential \
      bzip2 \
      ca-certificates \
      cmake \
      curl \
      dnsutils \
      fd-find \
      file \
      gh \
      git \
      git-lfs \
      gnupg \
      htop \
      iproute2 \
      iputils-ping \
      jq \
      less \
      libssl-dev \
      locales \
      make \
      nano \
      net-tools \
      nodejs \
      openssh-client \
      openssh-server \
      pkg-config \
      procps \
      psmisc \
      python-is-python3 \
      python3 \
      python3-dev \
      python3-pip \
      python3-venv \
      ripgrep \
      rsync \
      silversearcher-ag \
      socat \
      sqlite3 \
      sudo \
      tar \
      tmux \
      tree \
      unzip \
      vim \
      wget \
      xz-utils \
      zip \
      zsh && \
    rm -rf /var/lib/apt/lists/*

RUN case "${TARGETARCH}" in \
      amd64) GO_ARCH="amd64" ;; \
      arm64) GO_ARCH="arm64" ;; \
      *) echo "unsupported TARGETARCH: ${TARGETARCH}"; exit 1 ;; \
    esac && \
    curl -fsSL "https://go.dev/dl/go${GO_VERSION}.linux-${GO_ARCH}.tar.gz" | tar -C /usr/local -xz

RUN curl https://sh.rustup.rs -sSf | sh -s -- -y --profile minimal --default-toolchain "${RUST_TOOLCHAIN}" && \
    rustup component add clippy rustfmt && \
    python3 -m pip install --no-cache-dir --upgrade pip && \
    python3 -m pip install --no-cache-dir uv && \
    git lfs install --system && \
    ln -sf /usr/bin/fdfind /usr/local/bin/fd && \
    mkdir -p /run/sshd /workspace /etc/computer-mcp /var/lib/computer-mcp && \
    ssh-keygen -A && \
    sed -i 's/^#\\?PasswordAuthentication .*/PasswordAuthentication no/' /etc/ssh/sshd_config && \
    sed -i 's/^#\\?PermitRootLogin .*/PermitRootLogin yes/' /etc/ssh/sshd_config && \
    (sed -i 's@session\\s\\+required\\s\\+pam_loginuid.so@session optional pam_loginuid.so@' /etc/pam.d/sshd || true)

COPY --from=builder /workspace/target/release/computer-mcp /usr/local/bin/computer-mcp
COPY --from=builder /workspace/target/release/computer-mcpd /usr/local/bin/computer-mcpd
COPY --from=builder /workspace/target/release/computer-mcp-prd /usr/local/bin/computer-mcp-prd
COPY docker/runpod-entrypoint.sh /usr/local/bin/runpod-entrypoint

RUN chmod 0755 /usr/local/bin/computer-mcp \
               /usr/local/bin/computer-mcpd \
               /usr/local/bin/computer-mcp-prd \
               /usr/local/bin/runpod-entrypoint

WORKDIR /workspace

EXPOSE 22 443 8080

ENTRYPOINT ["/usr/local/bin/runpod-entrypoint"]
