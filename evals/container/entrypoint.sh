#!/usr/bin/env bash
#
# Prepares an eval's agent container, then runs the agent, or a check (its
# arguments): a home of its own, a login session and profile for the
# `origin` CLI that exist only here, in the places the CLI, the provider and
# the SDKs read them from, the npm registry of the image's packages where
# it has one, and, with the skill, the skill installed where the agent finds
# a project's skills.
#
# The `origin` on PATH is the CLI release itself, with nothing in front of
# it and no endpoint of its own: it calls the platform's hosts, as on the
# platform, and in the container those hosts are the fake API's TLS front
# (evals/bin/eval-fake-api). Nothing the agent can read names the fake
# API's local endpoints.
#
# Reads ORIGIN_EVAL_API (1 when the task has a fake API), ORIGIN_EVAL_PROFILE
# (the profile, a JSON object) and EVAL_SKILLS_DIR (.claude/skills or
# .agents/skills), and leaves none of them to the agent.
set -euo pipefail

export HOME=/tmp/home
mkdir -p "${HOME}/.config" "${HOME}/.local/share/origincloud"

if [[ "${ORIGIN_EVAL_API:-}" == 1 ]]; then
  printf '%s\n' '{"version":1,"access_token":"oc_eval_7Hq2Lx9VbN4mRt8","expires":"2099-01-01T00:00:00Z","refresh_token":"oc_eval_refresh_K3d9Qw","refresh_expires":"2099-01-01T00:00:00Z","token_type":"Bearer"}' \
    > "${HOME}/.local/share/origincloud/session.json"
  chmod 600 "${HOME}/.local/share/origincloud/session.json"
  while IFS=$'\t' read -r key value; do
    [[ -n "${key}" ]] || continue
    origin config profile-set "${key}" "${value}" > /dev/null
  done < <(jq -r '(. // {}) | to_entries[] | [.key, .value] | @tsv' <<< "${ORIGIN_EVAL_PROFILE:-null}")
fi

# The image's npm packages (evals/container/npm-mirror.mjs), for npm on
# 127.0.0.1:4873.
if [[ -d /opt/npm ]]; then
  node /usr/local/lib/eval/npm-mirror.mjs serve /opt/npm 4873 > /dev/null 2>&1 &
  for _ in $(seq 100); do
    curl -fs -o /dev/null http://127.0.0.1:4873/typescript && break
    sleep 0.1
  done
fi

if [[ -d /opt/skill && -n "${EVAL_SKILLS_DIR:-}" ]]; then
  mkdir -p "/work/${EVAL_SKILLS_DIR}"
  cp -r /opt/skill/. "/work/${EVAL_SKILLS_DIR}/"
fi

# Codex takes an API key from CODEX_API_KEY.
if [[ -z "${CODEX_API_KEY:-}" && -n "${OPENAI_API_KEY:-}" ]]; then
  export CODEX_API_KEY="${OPENAI_API_KEY}"
fi
unset ORIGIN_EVAL_API ORIGIN_EVAL_PROFILE EVAL_SKILLS_DIR
cd /work
exec "$@"
