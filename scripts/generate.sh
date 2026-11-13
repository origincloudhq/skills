#!/usr/bin/env bash
#
# Generates the skills' references (skills/*/references/{service}.md) and
# the generated parts of each skill's SKILL.md (its metadata, the shared
# rules and the map of its references) with this repository's generator
# (generator/), from:
#
# - the API definitions in origincloudhq/api at the commit API_REVISION
#   names;
# - the client releases clients.toml pins: origincloudhq/mcp's table of
#   one-way changes, the CLI's command tree, the Terraform provider's schema
#   and the SDKs' sources.
#
#   scripts/generate.sh           # generate them again
#   scripts/generate.sh --check   # check the checked-in files are current
#
# Both modes then check that every service of the API has a reference in
# every skill (scripts/coverage.sh).
#
# Needs cargo, git, curl, OpenTofu or Terraform (TF picks one; tofu is
# preferred) and network access.
set -euo pipefail

ROOT="$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"
readonly ROOT
readonly GITHUB="https://github.com/origincloudhq"

case "${1:-}" in
  "") mode="write" ;;
  --check) mode="check" ;;
  *) echo "usage: $0 [--check]" >&2; exit 2 ;;
esac

# The value of KEY in [SECTION] of clients.toml, a quoted string.
pin() {
  sed -n "/^\[$1\]/,/^\[/ s/^$2 = \"\(.*\)\"$/\1/p" "${ROOT}/clients.toml" | head -n 1
}
# Checks out REPOSITORY at REF (a commit or refs/tags/...) into DIR.
checkout() {
  git init --quiet "$3"
  git -C "$3" fetch --quiet --depth 1 "$1" "$2"
  git -C "$3" checkout --quiet FETCH_HEAD
}

revision="$(tr -d '[:space:]' < "${ROOT}/API_REVISION")"
[[ "${revision}" =~ ^[0-9a-f]{40}$ ]] || { echo "API_REVISION must hold a full commit hash" >&2; exit 1; }

tmp="$(mktemp -d)"
trap 'rm -rf "${tmp}"' EXIT
in="${tmp}/in"
mkdir -p "${in}"

checkout "${GITHUB}/api.git" "${revision}" "${in}/api"

# origincloudhq/mcp's table, from a commit generated from the same API.
mcp="$(pin mcp revision)"
[[ "${mcp}" =~ ^[0-9a-f]{40}$ ]] || { echo "clients.toml: mcp.revision must be a full commit hash" >&2; exit 1; }
checkout "${GITHUB}/mcp.git" "${mcp}" "${tmp}/mcp"
cp "${tmp}/mcp/generator/tools.toml" "${in}/tools.toml"
mcp_revision="$(tr -d '[:space:]' < "${tmp}/mcp/API_REVISION")"
if [[ "${mcp_revision}" != "${revision}" ]]; then
  echo "FAILED: origincloudhq/mcp@${mcp:0:12} was generated from origincloudhq/api@${mcp_revision:0:12}, not API_REVISION's ${revision:0:12}; run scripts/update.sh"
  exit 1
fi

# The CLI's command tree.
cli_version="$(pin cli version)"
commands="$(pin cli commands)"
if [[ "${commands}" == release ]]; then
  curl -fsSL --retry 3 -o "${in}/commands.json" \
    "${GITHUB}/cli/releases/download/v${cli_version}/commands.json"
else
  cp "${ROOT}/${commands}" "${in}/commands.json"
fi

# The provider's schema, and the API revision its release was generated from.
tf="${TF:-$(command -v tofu || command -v terraform || true)}"
[[ -n "${tf}" ]] || { echo "neither tofu nor terraform is installed" >&2; exit 1; }
source_address="$(pin terraform source)"
provider_version="$(pin terraform version)"
mkdir -p "${tmp}/tf"
cat > "${tmp}/tf/main.tf" <<HCL
terraform {
  required_providers {
    origincloud = {
      source  = "${source_address}"
      version = "${provider_version}"
    }
  }
}
HCL
(cd "${tmp}/tf" && "${tf}" init -input=false -no-color >/dev/null \
  && "${tf}" providers schema -json) > "${in}/terraform.json"
checkout "${GITHUB}/terraform-provider-origincloud.git" "refs/tags/v${provider_version}" "${tmp}/provider"
cp "${tmp}/provider/API_REVISION" "${in}/terraform-api-revision"

# The SDKs' sources at their releases.
for language in rust go typescript; do
  version="$(pin sdk "${language}")"
  [[ -n "${version}" ]] || { echo "clients.toml pins no sdk.${language}" >&2; exit 1; }
  checkout "${GITHUB}/sdk-${language}.git" "refs/tags/v${version}" "${in}/sdk-${language}"
done

(cd "${ROOT}" && cargo run --quiet --locked -p origincloud-skills-generator -- "${ROOT}" "${in}" "${tmp}/gen")

# The generated files, relative to the repository: each skill's SKILL.md and
# its references but the hand-written workflows.
generated() {
  [[ -d "$1/skills" ]] || return 0
  (cd "$1" && find skills -mindepth 2 -maxdepth 3 -type f -name '*.md' \
    \( -path 'skills/*/SKILL.md' -o -path 'skills/*/references/*.md' \) \
    ! -name '*-workflows.md') | LC_ALL=C sort
}

if [[ "${mode}" == write ]]; then
  # SKILL.md is written over, not removed: it is the generator's input too.
  { generated "${ROOT}" | grep -v '/SKILL.md$' || true; } | (cd "${ROOT}" && xargs -r rm -f)
  generated "${tmp}/gen" | (cd "${tmp}/gen" && xargs -r cp --parents -t "${ROOT}")
  echo "wrote $(generated "${ROOT}" | wc -l) files from origincloudhq/api@${revision:0:12}"
  "${ROOT}/scripts/coverage.sh" "${in}/api"
  exit 0
fi

failed=0
if ! diff <(generated "${ROOT}") <(generated "${tmp}/gen"); then
  failed=1
else
  while read -r file; do
    if ! cmp -s "${ROOT}/${file}" "${tmp}/gen/${file}"; then
      echo "differs: ${file}"
      failed=1
    fi
  done < <(generated "${ROOT}")
fi
if [[ "${failed}" -eq 1 ]]; then
  echo "FAILED: the generated files aren't what origincloudhq/api@${revision:0:12} and clients.toml generate; run $0, and never edit them by hand"
  exit 1
fi
echo "ok - the generated files are current (origincloudhq/api@${revision:0:12})"
"${ROOT}/scripts/coverage.sh" "${in}/api"
