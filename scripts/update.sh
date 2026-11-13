#!/usr/bin/env bash
#
# Moves the skills to the newest API definitions and client releases, then
# generates them again (scripts/generate.sh):
#
# - clients.toml's mcp.revision to the head of origincloudhq/mcp's main, and
#   API_REVISION to the API revision that commit was generated from: the
#   table of one-way changes leads, so that it always describes the API the
#   references are made from;
# - the CLI, the Terraform provider and each SDK to its newest release. The
#   CLI moves only while clients.toml takes its command tree from the
#   release (`commands = "release"`), and only to a release that carries
#   one.
#
# The regeneration workflow (.github/workflows/regenerate.yml) runs it; run
# it by hand the same way.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT
readonly GITHUB="https://github.com/origincloudhq"

cd "${ROOT}"

# The value of KEY in [SECTION] of clients.toml.
pin() {
  sed -n "/^\[$1\]/,/^\[/ s/^$2 = \"\(.*\)\"$/\1/p" clients.toml | head -n 1
}
# Sets KEY in [SECTION] of clients.toml to VALUE.
set_pin() {
  sed -i "/^\[$1\]/,/^\[/ s/^$2 = \".*\"$/$2 = \"$3\"/" clients.toml
  [[ "$(pin "$1" "$2")" == "$3" ]] || { echo "clients.toml: could not set $1.$2" >&2; exit 1; }
}
# The newest release version (vX.Y.Z, without the v) of REPOSITORY.
newest() {
  git ls-remote --tags --refs "${GITHUB}/$1.git" 'v*' | sed 's|.*refs/tags/v||' \
    | grep -E '^[0-9]+\.[0-9]+\.[0-9]+$' | sort -V | tail -n 1
}

mcp="$(git ls-remote "${GITHUB}/mcp.git" refs/heads/main | cut -f1)"
[[ "${mcp}" =~ ^[0-9a-f]{40}$ ]] || { echo "no main branch in ${GITHUB}/mcp" >&2; exit 1; }
tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT
git init --quiet "${tmp}/mcp"
git -C "${tmp}/mcp" fetch --quiet --depth 1 "${GITHUB}/mcp.git" "${mcp}"
revision="$(git -C "${tmp}/mcp" show FETCH_HEAD:API_REVISION | tr -d '[:space:]')"
[[ "${revision}" =~ ^[0-9a-f]{40}$ ]] || { echo "origincloudhq/mcp@${mcp:0:12} has no API_REVISION" >&2; exit 1; }
set_pin mcp revision "${mcp}"
echo "${revision}" > API_REVISION

if [[ "$(pin cli commands)" == release ]]; then
  cli="$(newest cli)"
  if curl -fsSLI --retry 3 -o /dev/null "${GITHUB}/cli/releases/download/v${cli}/commands.json"; then
    set_pin cli version "${cli}"
  else
    echo "origin ${cli} carries no commands.json; the CLI stays at $(pin cli version)" >&2
  fi
else
  echo "clients.toml takes the CLI's command tree from $(pin cli commands), not a release; the CLI stays at $(pin cli version)" >&2
fi
set_pin terraform version "$(newest terraform-provider-origincloud)"
for language in rust go typescript; do
  set_pin sdk "${language}" "$(newest "sdk-${language}")"
done

echo "origincloudhq/mcp@${mcp:0:12}, origincloudhq/api@${revision:0:12}," \
  "origin $(pin cli version), provider $(pin terraform version)," \
  "SDKs $(pin sdk rust) (Rust), $(pin sdk go) (Go), $(pin sdk typescript) (TypeScript)"
scripts/generate.sh
