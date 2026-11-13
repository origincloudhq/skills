# The image an eval's agent runs in (scripts/eval.sh), and its checks: one
# coding agent's CLI at a pinned version, the `origin` release under test,
# the fake API, the toolchains the skill's tasks need, and, in the target
# `with`, the skill. Built from the repository's root:
#
#   docker build -f evals/container/Dockerfile --target with \
#     --build-arg AGENT=claude --build-arg AGENT_VERSION=2.1.323 \
#     --build-arg ORIGIN_VERSION=1.19.0 --build-arg SKILL=origin .
#
# The container has no way to the internet but the model provider's API, so
# whatever a task builds with is in the image, at the versions clients.toml
# pins, for use offline:
#
# - origincloud-terraform: OpenTofu, which `terraform` names too, and the
#   provider release in a filesystem mirror, which `tofu init` installs from;
# - origincloud-sdk: Go, with the Go SDK's module and its dependencies in a
#   module proxy on disk; the npm packages of the TypeScript SDK, TypeScript
#   and Node's types in a registry on 127.0.0.1:4873 (npm-mirror.mjs); Rust,
#   with the Rust SDK's crates, Tokio and anyhow in Cargo's registry cache.
#
# scripts/eval.sh passes those versions as build arguments; the toolchains'
# own versions are pinned here.

# The skill whose tasks the image is for: its toolchains are the stage
# tools-${SKILL}.
ARG SKILL=origin

# The fake API, from this repository's source. Its Rust toolchain is the
# SDK image's too.
FROM rust:1.97.1-slim-trixie AS fake-api
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY generator generator
COPY evals/fake-api evals/fake-api
RUN cargo build --locked --release -p origincloud-skills-fake-api

FROM golang:1.27.4-trixie AS go

FROM node:22-trixie-slim AS base
RUN apt-get update \
  && apt-get install -y --no-install-recommends bash ca-certificates curl git jq openssl ripgrep socat unzip \
  && rm -rf /var/lib/apt/lists/*
# The platform's hosts, *.origincloudapis.eu, answer in the container as
# the fake API, through TLS that socat terminates (evals/bin/eval-fake-api):
# a certificate for them from a CA of this image's own, which the system,
# and so Go, Rust and the `origin` CLI, trusts, and Node with
# NODE_EXTRA_CA_CERTS. The key is no secret: it exists for the evals only.
RUN mkdir -p /opt/eval-tls && cd /opt/eval-tls \
  && openssl req -x509 -newkey rsa:2048 -nodes -days 3650 -subj "/CN=Origin Cloud evals CA" \
       -keyout ca.key -out ca.crt 2> /dev/null \
  && openssl req -newkey rsa:2048 -nodes -subj "/CN=origincloudapis.eu" \
       -keyout server.key -out server.csr 2> /dev/null \
  && printf 'subjectAltName=DNS:origincloudapis.eu,DNS:*.origincloudapis.eu,DNS:*.eu-central1.origincloudapis.eu\n' > san.ext \
  && openssl x509 -req -in server.csr -CA ca.crt -CAkey ca.key -CAcreateserial -days 3650 \
       -extfile san.ext -out server.crt 2> /dev/null \
  && rm -f ca.key server.csr san.ext ca.srl && chmod 644 server.key \
  && cp ca.crt /usr/local/share/ca-certificates/origincloud-evals.crt && update-ca-certificates > /dev/null
ENV NODE_EXTRA_CA_CERTS=/opt/eval-tls/ca.crt

# --- The toolchains of each skill's tasks. ---------------------------------
FROM base AS tools-origin

FROM base AS tools-origincloud-terraform
ARG OPENTOFU_VERSION=1.13.9
ARG PROVIDER_SOURCE
ARG PROVIDER_VERSION
# OpenTofu, checked against the release's checksums.
RUN arch="$(dpkg --print-architecture)" \
  && cd /tmp \
  && base="https://github.com/opentofu/opentofu/releases/download/v${OPENTOFU_VERSION}" \
  && curl -fsSLO "${base}/tofu_${OPENTOFU_VERSION}_linux_${arch}.zip" \
  && curl -fsSLO "${base}/tofu_${OPENTOFU_VERSION}_SHA256SUMS" \
  && sha256sum --check --ignore-missing "tofu_${OPENTOFU_VERSION}_SHA256SUMS" \
  && unzip -q "tofu_${OPENTOFU_VERSION}_linux_${arch}.zip" tofu -d /usr/local/bin \
  && ln -s tofu /usr/local/bin/terraform \
  && rm -f /tmp/tofu_* \
  && tofu version | grep -qF "v${OPENTOFU_VERSION}"
# The provider release, installed from the registry, which checks its
# signature, into a mirror under both registries' names, so that a source
# address naming either finds it.
RUN mkdir -p /tmp/mirror && cd /tmp/mirror \
  && printf 'terraform {\n  required_providers {\n    origincloud = {\n      source  = "%s"\n      version = "%s"\n    }\n  }\n}\n' \
       "${PROVIDER_SOURCE}" "${PROVIDER_VERSION}" > main.tf \
  && tofu providers mirror /opt/tofu/providers \
  && cp -R /opt/tofu/providers/registry.opentofu.org /opt/tofu/providers/registry.terraform.io \
  && chmod -R a+rX /opt/tofu && rm -rf /tmp/mirror
COPY evals/container/tofurc /opt/tofu/tofurc
ENV TF_CLI_CONFIG_FILE=/opt/tofu/tofurc TF_IN_AUTOMATION=1 CHECKPOINT_DISABLE=1

FROM base AS tools-origincloud-sdk
ARG SDK_GO_VERSION
ARG SDK_TYPESCRIPT_VERSION
ARG SDK_RUST_VERSION
RUN apt-get update \
  && apt-get install -y --no-install-recommends gcc libc6-dev make cmake pkg-config \
  && rm -rf /var/lib/apt/lists/*
# Go, and a module proxy on disk holding the Go SDK and every module it
# needs: `go get go.origincloud.eu/sdk` finds the pinned release, and only
# it.
COPY --from=go /usr/local/go /usr/local/go
ENV PATH=/usr/local/go/bin:${PATH} GOTOOLCHAIN=local
COPY evals/container/seed/go /tmp/seed-go
RUN cd /tmp/seed-go \
  && export GOPATH=/tmp/gopath GOMODCACHE=/tmp/gomod GOFLAGS=-modcacherw \
  && go mod init seed \
  && go get "go.origincloud.eu/sdk@v${SDK_GO_VERSION}" \
  && go mod tidy && go mod download all && go build ./... \
  && mkdir -p /opt/go && mv /tmp/gomod/cache/download /opt/go/proxy \
  && chmod -R a+rX /opt/go && rm -rf /tmp/seed-go /tmp/gopath /tmp/gomod /root/.cache
ENV GOPROXY=file:///opt/go/proxy GOSUMDB=off GOPATH=/tmp/go GOMODCACHE=/tmp/go/mod GOCACHE=/tmp/go/cache
# The TypeScript SDK's packages, TypeScript and Node's types, which the
# entrypoint serves.
COPY evals/container/npm-mirror.mjs /usr/local/lib/eval/npm-mirror.mjs
RUN set -- typescript@^5 typescript@latest @types/node@^20 @types/node@^22 @types/node@latest \
  && for p in billing compute core dns iam location network observability pvs resourcemanager sts support; do \
       set -- "$@" "@origincloud/${p}@${SDK_TYPESCRIPT_VERSION}"; \
     done \
  && node /usr/local/lib/eval/npm-mirror.mjs build /opt/npm "$@" \
  && chmod -R a+rX /opt/npm
ENV npm_config_registry=http://127.0.0.1:4873/ npm_config_audit=false npm_config_fund=false \
  npm_config_update_notifier=false
# Rust, and Cargo's registry cache holding the Rust SDK's crates and what
# they need, with Tokio and anyhow, which a program calling them uses. Cargo
# runs offline, and resolves to what the cache holds.
COPY --from=fake-api /usr/local/cargo /usr/local/cargo
COPY --from=fake-api /usr/local/rustup /usr/local/rustup
ENV CARGO_HOME=/usr/local/cargo RUSTUP_HOME=/usr/local/rustup PATH=/usr/local/cargo/bin:${PATH}
COPY evals/container/seed/rust /tmp/seed-rust
RUN cd /tmp/seed-rust \
  && sed -i "s/SDK_RUST_VERSION/${SDK_RUST_VERSION}/" Cargo.toml \
  && cargo build --quiet \
  && rm -rf /tmp/seed-rust \
  && chmod -R a+rwX /usr/local/cargo
ENV CARGO_NET_OFFLINE=true

# --- The agent, the CLI and the fake API. ----------------------------------
FROM tools-${SKILL} AS without
ARG AGENT
ARG AGENT_VERSION
ARG ORIGIN_VERSION
RUN case "${AGENT}" in \
      claude) npm install -g --registry=https://registry.npmjs.org/ "@anthropic-ai/claude-code@${AGENT_VERSION}" ;; \
      codex) npm install -g --registry=https://registry.npmjs.org/ "@openai/codex@${AGENT_VERSION}" ;; \
      *) echo "AGENT must be claude or codex" >&2; exit 1 ;; \
    esac \
  && npm cache clean --force
# The CLI release, checked against the release's checksums.
RUN arch="$(dpkg --print-architecture)" \
  && mkdir -p /opt/origin && cd /opt/origin \
  && curl -fsSLO "https://packages.origincloud.eu/cli/v${ORIGIN_VERSION}/origin-linux-${arch}" \
  && curl -fsSLO "https://packages.origincloud.eu/cli/v${ORIGIN_VERSION}/SHA256SUMS" \
  && sha256sum --check --ignore-missing SHA256SUMS \
  && mv "origin-linux-${arch}" origin && chmod 755 origin && rm SHA256SUMS \
  && ./origin --version | grep -qF " ${ORIGIN_VERSION}"
COPY --from=fake-api /src/target/release/fake-api /usr/local/bin/fake-api
COPY evals/container/entrypoint.sh /usr/local/bin/eval-entrypoint
COPY evals/bin/ /usr/local/bin/
# The entrypoint writes the `origin` the agent calls to /tmp/bin; a login
# shell, which resets PATH, finds it here too.
RUN ln -s /tmp/bin/origin /usr/local/bin/origin
ENTRYPOINT ["/usr/local/bin/eval-entrypoint"]

FROM without AS with
ARG SKILL
COPY skills/${SKILL} /opt/skill/${SKILL}
RUN rm -rf /opt/skill/*/evals && chmod -R a+rX /opt/skill
