#!/usr/bin/env bash
#
# Runs a skill's evals (skills/{skill}/evals/evals.json) with one coding
# agent, with the skill installed or without it, and checks what the agent
# did:
#
#   scripts/eval.sh --skill origin --agent claude|codex [--without] [--task ID] [--host]
#
# Each task runs in a directory of its own, holding the task's `files`; with
# the skill, it is installed where the agent finds a project's skills
# (.claude/skills/ for Claude Code, .agents/skills/ for Codex), without its
# evals. The agent gets the task's `prompt` non-interactively and no Origin
# Cloud credentials, so a task the agent can only finish by calling the API
# shows whether it stops and asks.
#
# By default the agent runs in a container (evals/container/Dockerfile)
# that holds only the agent's CLI at a pinned version, the `origin` release
# clients.toml pins, the fake API and, with the skill, the skill. Its only
# mount is the task's directory. Its network is a Docker network of its own
# for the run, with no way out but a proxy (evals/container/proxy.sh) that
# lets through HTTPS to the agent's model provider, api.anthropic.com or
# api.openai.com. In there, and only there, it may run any command without
# asking (Claude Code's bypassPermissions, Codex without its sandbox). Its
# credentials come from the file EVAL_CREDENTIALS, mode 0600, as KEY=VALUE
# lines: CLAUDE_CODE_OAUTH_TOKEN or ANTHROPIC_API_KEY for Claude Code,
# CODEX_API_KEY or OPENAI_API_KEY for Codex. They reach the container as
# environment variables, are never printed, and are blanked out of the
# transcripts.
#
# With --host, the agent runs on this machine instead, in a home directory
# of its own, with its usual permission prompts and its credentials from the
# environment (ANTHROPIC_API_KEY, CLAUDE_CODE_OAUTH_TOKEN or a
# CLAUDE_CONFIG_DIR logged in for the evals; CODEX_API_KEY or such a
# CODEX_HOME); the `origin` CLI is EVAL_ORIGIN, else the one on PATH. The
# container only carries what the `origin` skill's evals need, so the other
# skills' evals, which build Terraform, Go, Rust and TypeScript, run with
# --host.
#
# A task with an `api` runs the `origin` CLI against a fake API
# (evals/fake-api): `api.world` names a world file next to evals.json, the
# resources, canned answers and operations the fake serves, and the task's
# own `resources`, `responses` and `operations` are added to it, a resource
# replacing the world's of the same name; `profile` replaces the world's
# CLI profile (`default-project`, `default-zone`, …), `null` for none. The
# agent finds an `origin` on its PATH that is the real CLI with a login
# session of its own, sending every call to the fake API; the notice that
# the CLI calls a test server is left out of its output, so the agent sees
# what it would see on the platform.
#
# Then each check of the task runs in that directory:
#
# - `run`: the command succeeds;
# - `files_match` / `files_exclude`: a file matching `glob` has / no file
#   has a line matching the extended regular expression `pattern`;
# - `transcript_includes` / `transcript_excludes`: the agent's transcript
#   (its commands and messages) has / hasn't a match of `pattern`;
# - `answer_matches`: the agent's last message matches `pattern`;
# - `requests_include` / `requests_exclude`: the fake API received / didn't
#   receive a request whose line, `METHOD /path?query STATUS body`, matches
#   `pattern`.
#
# It prints a line of JSON per task, {"task", "agent", "skill", "passed",
# "failed": [the reasons of the checks that failed]}, and the directories
# are kept under EVAL_OUT (default: a new temporary directory) for reading
# the transcripts and the fake API's requests. EVAL_AGENT overrides the
# agent's command line, run in the task's directory (in the container,
# without --host) with the prompt as its last argument and the transcript
# on its standard output. EVAL_TIMEOUT bounds each task (default 900 s).
#
# Needs jq; without --host, Docker; with --host, the agent's CLI, and for
# the `origin` skill's evals the `origin` CLI and cargo.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT
# The agents' releases the container installs.
readonly CLAUDE_CODE_VERSION="2.1.323"
readonly CODEX_VERSION="0.144.4"

skill="" agent="" with=true only="" host=false
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skill) skill="$2"; shift 2 ;;
    --agent) agent="$2"; shift 2 ;;
    --without) with=false; shift ;;
    --task) only="$2"; shift 2 ;;
    --host) host=true; shift ;;
    *) echo "usage: $0 --skill NAME --agent claude|codex [--without] [--task ID] [--host]" >&2; exit 2 ;;
  esac
done
evals="${ROOT}/skills/${skill}/evals/evals.json"
[[ -f "${evals}" ]] || { echo "no evals for skill ${skill:-?}: ${evals}" >&2; exit 2; }
timeout="${EVAL_TIMEOUT:-900}"

case "${agent}" in
  claude)
    skills_dir=".claude/skills" version="${CLAUDE_CODE_VERSION}" provider="api.anthropic.com"
    credential_keys=(CLAUDE_CODE_OAUTH_TOKEN ANTHROPIC_API_KEY)
    command=(claude -p --output-format stream-json --verbose)
    in_container=(--permission-mode bypassPermissions) ;;
  codex)
    skills_dir=".agents/skills" version="${CODEX_VERSION}" provider="api.openai.com"
    credential_keys=(CODEX_API_KEY OPENAI_API_KEY)
    command=(codex exec --json --skip-git-repo-check)
    in_container=(--dangerously-bypass-approvals-and-sandbox) ;;
  *) echo "--agent must be claude or codex" >&2; exit 2 ;;
esac
if [[ "${host}" == false ]]; then
  command+=("${in_container[@]}")
fi
if [[ -n "${EVAL_AGENT:-}" ]]; then
  read -r -a command <<< "${EVAL_AGENT}"
fi

out="${EVAL_OUT:-$(mktemp -d)}"
mkdir -p "${out}"
variant="$([[ "${with}" == true ]] && echo with || echo without)"
pinned="$(sed -n '/^\[cli\]/,/^\[/s/^version = "\(.*\)"/\1/p' "${ROOT}/clients.toml")"
needs_api=false
if jq -e 'any(.tasks[]; has("api"))' "${evals}" > /dev/null; then
  needs_api=true
fi

# What runs at exit: stopping the fake API, the containers and the network.
cleanup=() fake_pid=""
stop_fake_api() {
  if [[ -n "${fake_pid}" ]]; then
    kill "${fake_pid}" 2> /dev/null || true
    wait "${fake_pid}" 2> /dev/null || true
    fake_pid=""
  fi
}
on_exit() {
  local c
  stop_fake_api
  for c in "${cleanup[@]+"${cleanup[@]}"}"; do eval "${c}" > /dev/null 2>&1 || true; done
}
trap on_exit EXIT

# --- The container: images, network, proxy and credentials. ---------------
run="" image="" credentials=()
declare -A secret=()
if [[ "${host}" == false ]]; then
  command -v docker > /dev/null || { echo "no docker: install it, or run with --host" >&2; exit 2; }

  # The credentials, read from their file into this shell only: the
  # container gets them by name (`docker run -e KEY`), never on a command
  # line, and nothing prints them.
  file="${EVAL_CREDENTIALS:-}"
  [[ -n "${file}" && -f "${file}" ]] || { echo "EVAL_CREDENTIALS names no file" >&2; exit 2; }
  [[ "$(stat -c %a "${file}")" == 600 ]] || { echo "${file} must be mode 0600" >&2; exit 2; }
  for key in "${credential_keys[@]}"; do
    line="$(grep -m 1 "^${key}=" "${file}" || true)"
    [[ -n "${line}" ]] || continue
    value="${line#*=}"
    value="${value%\"}" value="${value#\"}"
    [[ -n "${value}" ]] || continue
    secret["${key}"]="${value}"
    credentials+=(-e "${key}")
  done
  unset line value
  [[ ${#credentials[@]} -gt 0 ]] || {
    echo "${file} has none of ${credential_keys[*]} for ${agent}" >&2; exit 2; }

  image="origincloud-skills-eval-${agent}:${variant}"
  echo "building ${image} (${agent} ${version}, origin ${pinned})" >&2
  docker build --quiet -f "${ROOT}/evals/container/Dockerfile" --target "${variant}" \
    --build-arg AGENT="${agent}" --build-arg AGENT_VERSION="${version}" \
    --build-arg ORIGIN_VERSION="${pinned}" --build-arg SKILL="${skill}" \
    -t "${image}" "${ROOT}" > /dev/null
  docker build --quiet -f "${ROOT}/evals/container/proxy.Dockerfile" \
    -t origincloud-skills-eval-proxy "${ROOT}" > /dev/null

  # A network with no way out, and the proxy, the one container on it that
  # also reaches the internet.
  run="eval-${agent}-${variant}-$$"
  docker network create --internal "${run}" > /dev/null
  cleanup+=("docker network rm ${run}")
  docker run -d --name "${run}-proxy" --network "${run}" -e EVAL_ALLOW="${provider}" \
    origincloud-skills-eval-proxy > /dev/null
  cleanup=("docker rm -f ${run}-proxy" "${cleanup[@]}")
  docker network connect bridge "${run}-proxy"
fi

# --- On the host: the CLI and the fake API. --------------------------------
fake_api="" origin_bin=""
if [[ "${host}" == true ]]; then
  # The toolchains stay where they are installed when the home changes.
  for name in RUSTUP_HOME CARGO_HOME GOPATH; do
    case "${name}" in
      RUSTUP_HOME) default="${HOME}/.rustup" ;;
      CARGO_HOME) default="${HOME}/.cargo" ;;
      GOPATH) default="${HOME}/go" ;;
    esac
    if [[ -z "${!name:-}" && -d "${default}" ]]; then
      export "${name}=${default}"
    fi
  done
  if [[ "${needs_api}" == true ]]; then
    origin_bin="${EVAL_ORIGIN:-$(command -v origin || true)}"
    [[ -x "${origin_bin}" ]] || { echo "no origin CLI: set EVAL_ORIGIN" >&2; exit 2; }
    if ! "${origin_bin}" --version | grep -qF " ${pinned}"; then
      echo "warning: ${origin_bin} isn't origin ${pinned}, the release the skill describes" >&2
    fi
    fake_api="${EVAL_FAKE_API:-}"
    if [[ -z "${fake_api}" ]]; then
      fake_api="$(cd "${ROOT}" && cargo build --quiet --locked -p origincloud-skills-fake-api \
        --message-format=json | jq -r 'select(.executable != null) | .executable' | tail -n 1)"
    fi
  fi
fi

# Writes a task's world: the world file with the task's own additions.
write_world() {
  local api="$1" target="$2" world
  world="$(dirname "${evals}")/$(jq -r .world <<< "${api}")"
  jq --argjson t "${api}" '
    ($t.resources // []) as $own
    | .resources = ([(.resources // [])[] | select(.name as $n | ($own | map(.name) | index($n)) | not)] + $own)
    | .responses = (($t.responses // []) + (.responses // []))
    | .operations = ((.operations // []) + ($t.operations // []))
    | if ($t | has("profile")) then .profile = $t.profile else . end
  ' "${world}" > "${target}"
}

# Waits for the fake API to write its endpoints.
await_endpoints() {
  local dir="$1"
  for _ in $(seq 300); do
    [[ -s "${dir}/api/endpoints.env" ]] && return 0
    sleep 0.1
  done
  echo "the fake API didn't start: ${dir}/api/stderr.log" >&2
  exit 1
}

# On the host: starts the fake API for a task, and puts an `origin` that
# calls it first on the agent's PATH, in ${dir}/bin.
start_fake_api_on_host() {
  local dir="$1"
  mkdir -p "${dir}/bin" "${dir}/origin/config" "${dir}/origin/data/origincloud"
  "${fake_api}" --world "${dir}/api/world.json" --log "${dir}/api/requests.log" \
    --env "${dir}/api/endpoints.env" 2> "${dir}/api/stderr.log" &
  fake_pid=$!
  await_endpoints "${dir}"

  # A login session that is valid until 2099, of a token that exists only
  # here, and the world's profile.
  printf '%s\n' '{"version":1,"access_token":"oc_eval_7Hq2Lx9VbN4mRt8","expires":"2099-01-01T00:00:00Z","refresh_token":"oc_eval_refresh_K3d9Qw","refresh_expires":"2099-01-01T00:00:00Z","token_type":"Bearer"}' \
    > "${dir}/origin/data/origincloud/session.json"
  chmod 600 "${dir}/origin/data/origincloud/session.json"
  while IFS=$'\t' read -r key value; do
    [[ -n "${key}" ]] || continue
    XDG_CONFIG_HOME="${dir}/origin/config" "${origin_bin}" config profile-set "${key}" "${value}" > /dev/null
  done < <(jq -r '(.profile // {}) | to_entries[] | [.key, .value] | @tsv' "${dir}/api/world.json")

  {
    echo '#!/usr/bin/env bash'
    printf 'export XDG_CONFIG_HOME=%q XDG_DATA_HOME=%q\n' "${dir}/origin/config" "${dir}/origin/data"
    sed 's/^/export /' "${dir}/api/endpoints.env"
    printf 'origin_bin=%q\n' "${origin_bin}"
    cat <<'WRAPPER'
err="$(mktemp)"
trap 'rm -f "${err}"' EXIT
"${origin_bin}" "$@" 2> "${err}"
status=$?
grep -v -e '^Sending the `origin [a-z]*` calls to http://127' -e '^Looking up the zones at http://127' "${err}" >&2
exit "${status}"
WRAPPER
  } > "${dir}/bin/origin"
  chmod +x "${dir}/bin/origin"
}

# Runs the agent on the host.
run_on_host() {
  local dir="$1" prompt="$2" has_api="$3" path="${PATH}"
  if [[ "${with}" == true ]]; then
    mkdir -p "${dir}/work/${skills_dir}"
    cp -r "${ROOT}/skills/${skill}" "${dir}/work/${skills_dir}/${skill}"
    rm -rf "${dir}/work/${skills_dir}/${skill}/evals"
  fi
  if [[ "${has_api}" == true ]]; then
    start_fake_api_on_host "${dir}"
    path="${dir}/bin:${PATH}"
  fi
  mkdir -p "${dir}/home"
  (
    cd "${dir}/work"
    # No Origin Cloud credentials reach the agent, nor the user's home.
    while read -r name; do unset "${name}"; done < <(env | sed -n 's/^\(ORIGINCLOUD_[A-Z0-9_]*\)=.*/\1/p')
    unset XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME XDG_CACHE_HOME
    export HOME="${dir}/home" PATH="${path}"
    timeout "${timeout}" "${command[@]}" "${prompt}"
  ) > "${dir}/transcript.jsonl" 2> "${dir}/stderr.log" < /dev/null || true
  stop_fake_api
}

# Runs the agent in its container. The fake API runs in a container of its
# own, from the same image, whose network the agent's container shares, so
# that the agent reaches it on 127.0.0.1 and nothing else but the proxy.
run_in_container() {
  local dir="$1" prompt="$2" has_api="$3" id="$4" net endpoints="" profile="null" name
  name="${run}-$(tr -c 'a-z0-9-' '-' <<< "${id}" | head -c 40)"
  net=(--network "${run}")
  if [[ "${has_api}" == true ]]; then
    docker run -d --name "${name}-api" --network "${run}" --user "$(id -u):$(id -g)" \
      -v "${dir}/api:/api" --entrypoint /usr/local/bin/fake-api "${image}" \
      --world /api/world.json --log /api/requests.log --env /api/endpoints.env > /dev/null
    cleanup=("docker rm -f ${name}-api" "${cleanup[@]}")
    docker logs -f "${name}-api" > "${dir}/api/stderr.log" 2>&1 &
    await_endpoints "${dir}"
    net=(--network "container:${name}-api")
    endpoints="$(cat "${dir}/api/endpoints.env")"
    profile="$(jq -c '.profile' "${dir}/api/world.json")"
  fi
  (
    for key in "${!secret[@]}"; do export "${key}=${secret[${key}]}"; done
    export ORIGIN_EVAL_ENDPOINTS="${endpoints}" ORIGIN_EVAL_PROFILE="${profile}" \
      EVAL_SKILLS_DIR="${skills_dir}"
    timeout "${timeout}" docker run --rm --name "${name}" "${net[@]}" \
      --user "$(id -u):$(id -g)" -v "${dir}/work:/work" \
      -e HTTPS_PROXY="http://${run}-proxy:3128" -e https_proxy="http://${run}-proxy:3128" \
      -e NO_PROXY=127.0.0.1,localhost -e no_proxy=127.0.0.1,localhost \
      -e CLAUDE_CODE_DISABLE_NONESSENTIAL_TRAFFIC=1 -e DISABLE_AUTOUPDATER=1 \
      -e ORIGIN_EVAL_ENDPOINTS -e ORIGIN_EVAL_PROFILE -e EVAL_SKILLS_DIR \
      "${credentials[@]}" "${image}" "${command[@]}" "${prompt}"
  ) > "${dir}/transcript.jsonl" 2> "${dir}/stderr.log" < /dev/null || true
  docker rm -f "${name}" > /dev/null 2>&1 || true
  if [[ "${has_api}" == true ]]; then
    docker rm -f "${name}-api" > /dev/null 2>&1 || true
    cleanup=("${cleanup[@]:1}")
  fi
  # Blank out any credential the agent printed.
  (
    for key in "${!secret[@]}"; do export "${key}=${secret[${key}]}"; done
    EVAL_REDACT="${!secret[*]}" perl -pi -e \
      'for my $k (split / /, $ENV{EVAL_REDACT}) { my $v = $ENV{$k}; s/\Q$v\E/[redacted]/g if length $v }' \
      "${dir}/transcript.jsonl" "${dir}/stderr.log"
  )
}

while read -r task; do
  id="$(jq -r .id <<< "${task}")"
  [[ -z "${only}" || "${only}" == "${id}" ]] || continue
  dir="${out}/${agent}/${variant}/${id}"
  rm -rf "${dir}"
  mkdir -p "${dir}/work"
  # The task's files.
  while read -r name; do
    [[ -n "${name}" ]] || continue
    mkdir -p "$(dirname "${dir}/work/${name}")"
    jq -r --arg n "${name}" '.files[$n]' <<< "${task}" > "${dir}/work/${name}"
  done < <(jq -r '(.files // {}) | keys[]' <<< "${task}")
  has_api=false
  if jq -e 'has("api")' <<< "${task}" > /dev/null; then
    has_api=true
    mkdir -p "${dir}/api"
    write_world "$(jq -c .api <<< "${task}")" "${dir}/api/world.json"
  fi

  prompt="$(jq -r .prompt <<< "${task}")"
  if [[ "${host}" == true ]]; then
    run_on_host "${dir}" "${prompt}" "${has_api}"
  else
    run_in_container "${dir}" "${prompt}" "${has_api}" "${id}"
  fi
  # The last message: Claude Code's `result`, Codex's last agent message;
  # the transcript's last line otherwise.
  answer="$(jq -rs '[.[] | select(.type == "result") | .result] | last // empty' "${dir}/transcript.jsonl" 2>/dev/null || true)"
  if [[ -z "${answer}" ]]; then
    answer="$(jq -rs '[.[] | (.item // .msg // {}) | select(.type == "agent_message") | .text // .message] | last // empty' "${dir}/transcript.jsonl" 2>/dev/null || true)"
  fi
  [[ -n "${answer}" ]] || answer="$(tail -n 1 "${dir}/transcript.jsonl")"
  printf '%s\n' "${answer}" > "${dir}/answer.txt"
  requests="${dir}/api/requests.log"
  [[ -f "${requests}" ]] || requests=/dev/null

  failed=()
  while read -r check; do
    kind="$(jq -r .kind <<< "${check}")"
    pattern="$(jq -r '.pattern // ""' <<< "${check}")"
    glob="$(jq -r '.glob // "*"' <<< "${check}")"
    why="$(jq -r .why <<< "${check}")"
    ok=false
    case "${kind}" in
      run)
        if (cd "${dir}/work" && bash -c "$(jq -r .command <<< "${check}")") >> "${dir}/checks.log" 2>&1; then ok=true; fi ;;
      files_match)
        if find "${dir}/work" -path "${dir}/work/${skills_dir}" -prune -o -type f -name "${glob}" -print0 \
          | xargs -0 -r grep -qE -- "${pattern}"; then ok=true; fi ;;
      files_exclude)
        if ! find "${dir}/work" -path "${dir}/work/${skills_dir}" -prune -o -type f -name "${glob}" -print0 \
          | xargs -0 -r grep -qE -- "${pattern}"; then ok=true; fi ;;
      transcript_includes)
        if grep -qE -- "${pattern}" "${dir}/transcript.jsonl"; then ok=true; fi ;;
      transcript_excludes)
        if ! grep -qE -- "${pattern}" "${dir}/transcript.jsonl"; then ok=true; fi ;;
      answer_matches)
        if grep -qP -- "${pattern}" "${dir}/answer.txt"; then ok=true; fi ;;
      requests_include)
        if grep -qE -- "${pattern}" "${requests}"; then ok=true; fi ;;
      requests_exclude)
        if ! grep -qE -- "${pattern}" "${requests}"; then ok=true; fi ;;
      *) echo "${id}: unknown check ${kind}" >&2; exit 2 ;;
    esac
    [[ "${ok}" == true ]] || failed+=("${kind}: ${why}")
  done < <(jq -c '.checks[]' <<< "${task}")

  jq -cn --arg task "${id}" --arg agent "${agent}" --arg skill "${variant}" \
    --argjson failed "$(printf '%s\n' "${failed[@]+"${failed[@]}"}" | jq -R . | jq -sc 'map(select(length > 0))')" \
    '{task: $task, agent: $agent, skill: $skill, passed: ($failed | length == 0), failed: $failed}'
done < <(jq -c '.tasks[]' "${evals}")
echo "transcripts: ${out}" >&2
