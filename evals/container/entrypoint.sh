#!/usr/bin/env bash
#
# Prepares an eval's agent container, then runs the agent (its arguments):
# a home of its own, a login session and profile for the `origin` CLI that
# exist only here, an `origin` on PATH that calls the fake API, and, with
# the skill, the skill installed where the agent finds a project's skills.
#
# Reads ORIGIN_EVAL_ENDPOINTS (the fake API's ORIGINCLOUD_*_ENDPOINT lines),
# ORIGIN_EVAL_PROFILE (the profile, a JSON object) and EVAL_SKILLS_DIR
# (.claude/skills or .agents/skills), and leaves none of them to the agent.
set -euo pipefail

export HOME=/tmp/home
mkdir -p "${HOME}" /tmp/bin /tmp/origin/config /tmp/origin/data/origincloud

if [[ -n "${ORIGIN_EVAL_ENDPOINTS:-}" ]]; then
  printf '%s\n' '{"version":1,"access_token":"oc_eval_7Hq2Lx9VbN4mRt8","expires":"2099-01-01T00:00:00Z","refresh_token":"oc_eval_refresh_K3d9Qw","refresh_expires":"2099-01-01T00:00:00Z","token_type":"Bearer"}' \
    > /tmp/origin/data/origincloud/session.json
  chmod 600 /tmp/origin/data/origincloud/session.json
  while IFS=$'\t' read -r key value; do
    [[ -n "${key}" ]] || continue
    XDG_CONFIG_HOME=/tmp/origin/config /opt/origin/origin config profile-set "${key}" "${value}" > /dev/null
  done < <(jq -r '(. // {}) | to_entries[] | [.key, .value] | @tsv' <<< "${ORIGIN_EVAL_PROFILE:-null}")
  {
    echo '#!/usr/bin/env bash'
    echo 'export XDG_CONFIG_HOME=/tmp/origin/config XDG_DATA_HOME=/tmp/origin/data'
    echo 'unset HTTPS_PROXY https_proxy HTTP_PROXY http_proxy'
    sed -n 's/^\(ORIGINCLOUD_[A-Z]*_ENDPOINT=http:\/\/127\.0\.0\.1:[0-9]*\)$/export \1/p' <<< "${ORIGIN_EVAL_ENDPOINTS}"
    cat <<'WRAPPER'
err="$(mktemp)"
trap 'rm -f "${err}"' EXIT
/opt/origin/origin "$@" 2> "${err}"
status=$?
grep -v -e '^Sending the `origin [a-z]*` calls to http://127' -e '^Looking up the zones at http://127' "${err}" >&2
exit "${status}"
WRAPPER
  } > /tmp/bin/origin
  chmod 755 /tmp/bin/origin
fi
export PATH="/tmp/bin:${PATH}"

if [[ -d /opt/skill && -n "${EVAL_SKILLS_DIR:-}" ]]; then
  mkdir -p "/work/${EVAL_SKILLS_DIR}"
  cp -r /opt/skill/. "/work/${EVAL_SKILLS_DIR}/"
fi

# Codex takes an API key from CODEX_API_KEY.
if [[ -z "${CODEX_API_KEY:-}" && -n "${OPENAI_API_KEY:-}" ]]; then
  export CODEX_API_KEY="${OPENAI_API_KEY}"
fi
unset ORIGIN_EVAL_ENDPOINTS ORIGIN_EVAL_PROFILE EVAL_SKILLS_DIR
cd /work
exec "$@"
