#!/usr/bin/env bash
#
# Checks that every service of the API has a reference in every skill:
# skills/{skill}/references/{service}.md, linked from the skill's SKILL.md.
# The services are read from the API checkout API_DIR, apart from the
# generator: each directory origincloud/{service}/ with a proto file that
# defines a service, but the operations (longrunning), whose methods every
# service's reference covers.
#
#   scripts/coverage.sh API_DIR
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT
api="${1:?usage: $0 API_DIR}"

services=()
for dir in "${api}"/origincloud/*/; do
  service="$(basename "${dir}")"
  [[ "${service}" == longrunning ]] && continue
  if grep -rqsE '^service [A-Za-z]+ *\{' --include='*.proto' "${dir}"; then
    services+=("${service}")
  fi
done
[[ "${#services[@]}" -gt 0 ]] || { echo "no services in ${api}" >&2; exit 1; }

skills=()
for skill in "${ROOT}"/skills/*/; do
  skills+=("$(basename "${skill}")")
done
[[ "${#skills[@]}" -gt 0 ]] || { echo "no skills in ${ROOT}/skills" >&2; exit 1; }

missing=0
for skill in "${skills[@]}"; do
  for service in "${services[@]}"; do
    reference="skills/${skill}/references/${service}.md"
    if [[ ! -s "${ROOT}/${reference}" ]]; then
      echo "missing: ${reference}"
      missing=1
    elif ! grep -qF "(references/${service}.md)" "${ROOT}/skills/${skill}/SKILL.md"; then
      echo "not linked from skills/${skill}/SKILL.md: ${reference}"
      missing=1
    fi
  done
done
if [[ "${missing}" -eq 1 ]]; then
  echo "FAILED: a service of the API has no reference in every skill"
  exit 1
fi
echo "ok - ${#services[@]} services have a reference in each of ${#skills[@]} skills"
