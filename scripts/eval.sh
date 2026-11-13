#!/usr/bin/env bash
#
# Runs a skill's evals (skills/{skill}/evals/evals.json) with one coding
# agent, with the skill installed or without it, and checks what the agent
# did:
#
#   scripts/eval.sh --skill origincloud-terraform --agent claude [--without] [--task ID]
#
# Each task runs in a directory of its own, holding the task's `files`; with
# the skill, it is installed where the agent finds a project's skills
# (.claude/skills/ for Claude Code, .agents/skills/ for Codex), without its
# evals. The agent gets the task's `prompt` non-interactively, a home
# directory of its own, so that neither the user's own skills, settings and
# sessions nor anything the agent writes there mixes with the run, and no
# Origin Cloud credentials: the environment's ORIGINCLOUD_* variables are
# removed, so a task the agent can only finish by calling the API shows
# whether it stops and asks.
#
# A task with an `api` runs the `origin` CLI against a fake API instead
# (evals/fake-api): `api.world` names a world file next to evals.json, the
# resources, canned answers and operations the fake serves, and the task's
# own `resources`, `responses` and `operations` are added to it, a resource
# replacing the world's of the same name; `profile` replaces the world's
# CLI profile (`default-project`, `default-zone`, …), `null` for none. The
# agent finds an `origin` on its PATH that is the real CLI (EVAL_ORIGIN, else
# the `origin` on PATH) with a login session of its own, sending every call
# to the fake API; the notice that the CLI calls a test server is left out
# of its output, so the agent sees what it would see on the platform.
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
# agent's command line, a command run in the task's directory with the
# prompt as its last argument and the transcript on its standard output.
#
# Needs jq, the agent's CLI (`claude` or `codex`) and its credentials in the
# environment, since the agent runs in a home of its own: for Claude Code
# ANTHROPIC_API_KEY, CLAUDE_CODE_OAUTH_TOKEN (`claude setup-token`) or
# CLAUDE_CONFIG_DIR naming a directory logged in for the evals; for Codex
# CODEX_API_KEY or CODEX_HOME naming such a directory. The `origin` skill's
# evals also need the `origin` CLI and cargo, which builds the fake API.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT

skill="" agent="" with=true only=""
while [[ $# -gt 0 ]]; do
  case "$1" in
    --skill) skill="$2"; shift 2 ;;
    --agent) agent="$2"; shift 2 ;;
    --without) with=false; shift ;;
    --task) only="$2"; shift 2 ;;
    *) echo "usage: $0 --skill NAME --agent claude|codex [--without] [--task ID]" >&2; exit 2 ;;
  esac
done
evals="${ROOT}/skills/${skill}/evals/evals.json"
[[ -f "${evals}" ]] || { echo "no evals for skill ${skill:-?}: ${evals}" >&2; exit 2; }

# The agents run unattended in a throwaway directory and home: they may run
# commands and write files there without asking.
case "${agent}" in
  claude)
    skills_dir=".claude/skills"
    command=(claude -p --output-format stream-json --verbose --permission-mode bypassPermissions) ;;
  codex)
    skills_dir=".agents/skills"
    command=(codex exec --json --skip-git-repo-check --sandbox workspace-write
      -c sandbox_workspace_write.network_access=true) ;;
  *) echo "--agent must be claude or codex" >&2; exit 2 ;;
esac
if [[ -n "${EVAL_AGENT:-}" ]]; then
  read -r -a command <<< "${EVAL_AGENT}"
fi

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

out="${EVAL_OUT:-$(mktemp -d)}"
mkdir -p "${out}"
variant="$([[ "${with}" == true ]] && echo with || echo without)"

# The fake API and the CLI, for a skill whose tasks call the API.
fake_api="" origin_bin="" fake_pid=""
if jq -e 'any(.tasks[]; has("api"))' "${evals}" > /dev/null; then
  origin_bin="${EVAL_ORIGIN:-$(command -v origin || true)}"
  [[ -x "${origin_bin}" ]] || { echo "no origin CLI: set EVAL_ORIGIN" >&2; exit 2; }
  pinned="$(sed -n '/^\[cli\]/,/^\[/s/^version = "\(.*\)"/\1/p' "${ROOT}/clients.toml")"
  if ! "${origin_bin}" --version | grep -qF " ${pinned}"; then
    echo "warning: ${origin_bin} isn't origin ${pinned}, the release the skill describes" >&2
  fi
  fake_api="${EVAL_FAKE_API:-}"
  if [[ -z "${fake_api}" ]]; then
    fake_api="$(cd "${ROOT}" && cargo build --quiet --locked -p origincloud-skills-fake-api \
      --message-format=json | jq -r 'select(.executable != null) | .executable' | tail -n 1)"
  fi
fi
stop_fake_api() {
  if [[ -n "${fake_pid}" ]]; then
    kill "${fake_pid}" 2> /dev/null || true
    wait "${fake_pid}" 2> /dev/null || true
    fake_pid=""
  fi
}
trap stop_fake_api EXIT

# Starts the fake API for a task, with its world, and puts an `origin` that
# calls it first on the agent's PATH, in ${dir}/bin.
start_fake_api() {
  local task="$1" dir="$2" api world
  api="$(jq -c .api <<< "${task}")"
  world="$(dirname "${evals}")/$(jq -r .world <<< "${api}")"
  mkdir -p "${dir}/api" "${dir}/bin" "${dir}/origin/config" "${dir}/origin/data/origincloud"
  jq --argjson t "${api}" '
    ($t.resources // []) as $own
    | .resources = ([(.resources // [])[] | select(.name as $n | ($own | map(.name) | index($n)) | not)] + $own)
    | .responses = (($t.responses // []) + (.responses // []))
    | .operations = ((.operations // []) + ($t.operations // []))
    | if ($t | has("profile")) then .profile = $t.profile else . end
  ' "${world}" > "${dir}/api/world.json"
  "${fake_api}" --world "${dir}/api/world.json" --log "${dir}/api/requests.log" \
    --env "${dir}/api/endpoints.env" 2> "${dir}/api/stderr.log" &
  fake_pid=$!
  for _ in $(seq 100); do
    [[ -f "${dir}/api/endpoints.env" ]] && break
    sleep 0.1
  done
  [[ -f "${dir}/api/endpoints.env" ]] || { echo "the fake API didn't start: ${dir}/api/stderr.log" >&2; exit 1; }

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

while read -r task; do
  id="$(jq -r .id <<< "${task}")"
  [[ -z "${only}" || "${only}" == "${id}" ]] || continue
  dir="${out}/${agent}/${variant}/${id}"
  rm -rf "${dir}"
  mkdir -p "${dir}/work" "${dir}/home"
  # The task's files.
  while read -r name; do
    [[ -n "${name}" ]] || continue
    mkdir -p "$(dirname "${dir}/work/${name}")"
    jq -r --arg n "${name}" '.files[$n]' <<< "${task}" > "${dir}/work/${name}"
  done < <(jq -r '(.files // {}) | keys[]' <<< "${task}")
  if [[ "${with}" == true ]]; then
    mkdir -p "${dir}/work/${skills_dir}"
    cp -r "${ROOT}/skills/${skill}" "${dir}/work/${skills_dir}/${skill}"
    rm -rf "${dir}/work/${skills_dir}/${skill}/evals"
  fi
  path="${PATH}"
  if jq -e 'has("api")' <<< "${task}" > /dev/null; then
    start_fake_api "${task}" "${dir}"
    path="${dir}/bin:${PATH}"
  fi

  prompt="$(jq -r .prompt <<< "${task}")"
  (
    cd "${dir}/work"
    # No Origin Cloud credentials reach the agent, nor the user's home.
    while read -r name; do unset "${name}"; done < <(env | sed -n 's/^\(ORIGINCLOUD_[A-Z0-9_]*\)=.*/\1/p')
    unset XDG_CONFIG_HOME XDG_DATA_HOME XDG_STATE_HOME XDG_CACHE_HOME
    export HOME="${dir}/home" PATH="${path}"
    "${command[@]}" "${prompt}"
  ) > "${dir}/transcript.jsonl" 2> "${dir}/stderr.log" < /dev/null || true
  stop_fake_api
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
