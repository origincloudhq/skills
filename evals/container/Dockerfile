# The image an eval's agent runs in (scripts/eval.sh): one coding agent's
# CLI at a pinned version, the `origin` release under test, the fake API,
# and, in the target `with`, the skill. Built from the repository's root:
#
#   docker build -f evals/container/Dockerfile --target with \
#     --build-arg AGENT=claude --build-arg AGENT_VERSION=2.1.323 \
#     --build-arg ORIGIN_VERSION=1.19.0 --build-arg SKILL=origin .

# The fake API, from this repository's source.
FROM rust:1.97.1-slim-trixie AS fake-api
WORKDIR /src
COPY Cargo.toml Cargo.lock rust-toolchain.toml ./
COPY generator generator
COPY evals/fake-api evals/fake-api
RUN cargo build --locked --release -p origincloud-skills-fake-api

FROM node:22-trixie-slim AS without
ARG AGENT
ARG AGENT_VERSION
ARG ORIGIN_VERSION
RUN apt-get update \
  && apt-get install -y --no-install-recommends bash ca-certificates curl git jq ripgrep \
  && rm -rf /var/lib/apt/lists/*
RUN case "${AGENT}" in \
      claude) npm install -g "@anthropic-ai/claude-code@${AGENT_VERSION}" ;; \
      codex) npm install -g "@openai/codex@${AGENT_VERSION}" ;; \
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
ENTRYPOINT ["/usr/local/bin/eval-entrypoint"]

FROM without AS with
ARG SKILL
COPY skills/${SKILL} /opt/skill/${SKILL}
RUN rm -rf /opt/skill/*/evals && chmod -R a+rX /opt/skill
