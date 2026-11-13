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
# (.claude/skills/ for Claude Code, .agents/skills/ for Codex). The agent
# gets the task's `prompt` non-interactively and no Origin Cloud
# credentials: the environment's ORIGINCLOUD_* variables are removed, so a
# task the agent can only finish by calling the API shows whether it stops
# and asks. Then each check of the task runs in that directory:
#
# - `run`: the command succeeds;
# - `files_match` / `files_exclude`: a file matching `glob` has / no file
#   has a line matching the extended regular expression `pattern`;
# - `transcript_includes` / `transcript_excludes`: the agent's transcript
#   (its commands and messages) has / hasn't a match of `pattern`;
# - `answer_matches`: the agent's last message matches `pattern`.
#
# It prints a line of JSON per task, {"task", "agent", "skill", "passed",
# "failed": [the reasons of the checks that failed]}, and the directories
# are kept under EVAL_OUT (default: a new temporary directory) for reading
# the transcripts. EVAL_AGENT overrides the agent's command line, a command
# run in the task's directory with the prompt as its last argument and the
# transcript on its standard output.
#
# Needs jq, and the agent's CLI (`claude` or `codex`) logged in.
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

case "${agent}" in
  claude) skills_dir=".claude/skills"; command=(claude -p --output-format stream-json --verbose) ;;
  codex) skills_dir=".agents/skills"; command=(codex exec --json --skip-git-repo-check) ;;
  *) echo "--agent must be claude or codex" >&2; exit 2 ;;
esac
if [[ -n "${EVAL_AGENT:-}" ]]; then
  read -r -a command <<< "${EVAL_AGENT}"
fi

out="${EVAL_OUT:-$(mktemp -d)}"
mkdir -p "${out}"
variant="$([[ "${with}" == true ]] && echo with || echo without)"

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
  if [[ "${with}" == true ]]; then
    mkdir -p "${dir}/work/${skills_dir}"
    cp -r "${ROOT}/skills/${skill}" "${dir}/work/${skills_dir}/${skill}"
  fi

  prompt="$(jq -r .prompt <<< "${task}")"
  (
    cd "${dir}/work"
    # No Origin Cloud credentials reach the agent.
    while read -r name; do unset "${name}"; done < <(env | sed -n 's/^\(ORIGINCLOUD_[A-Z0-9_]*\)=.*/\1/p')
    "${command[@]}" "${prompt}"
  ) > "${dir}/transcript.jsonl" 2> "${dir}/stderr.log" || true
  # The last message: Claude Code's `result`, Codex's last agent message;
  # the transcript's last line otherwise.
  answer="$(jq -rs '[.[] | select(.type == "result") | .result] | last // empty' "${dir}/transcript.jsonl" 2>/dev/null || true)"
  if [[ -z "${answer}" ]]; then
    answer="$(jq -rs '[.[] | (.item // .msg // {}) | select(.type == "agent_message") | .text // .message] | last // empty' "${dir}/transcript.jsonl" 2>/dev/null || true)"
  fi
  [[ -n "${answer}" ]] || answer="$(tail -n 1 "${dir}/transcript.jsonl")"
  printf '%s\n' "${answer}" > "${dir}/answer.txt"

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
      *) echo "${id}: unknown check ${kind}" >&2; exit 2 ;;
    esac
    [[ "${ok}" == true ]] || failed+=("${kind}: ${why}")
  done < <(jq -c '.checks[]' <<< "${task}")

  jq -cn --arg task "${id}" --arg agent "${agent}" --arg skill "${variant}" \
    --argjson failed "$(printf '%s\n' "${failed[@]+"${failed[@]}"}" | jq -R . | jq -sc 'map(select(length > 0))')" \
    '{task: $task, agent: $agent, skill: $skill, passed: ($failed | length == 0), failed: $failed}'
done < <(jq -c '.tasks[]' "${evals}")
echo "transcripts: ${out}" >&2
