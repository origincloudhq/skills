# Contributing

Thank you for helping improve Origin Cloud's agent skills.

## Issues

Open an issue for anything that is wrong, unclear or missing: a skill that
leads an agent astray, a reference that names something the client doesn't
have, a rule an agent keeps breaking. Say which skill and agent it is about,
and include the prompt and what the agent did where you can, with tokens and
account details removed.

To report a security vulnerability, don't open an issue: use **Report a
vulnerability** on this repository's Security tab, which reaches us
privately.

## Pull requests

Pull requests are reviewed and merged here. For anything larger than a small
fix, open an issue first, so that we can agree on the change before you spend
time on it. CI must pass: the checks below, run on every pull request.

## How the skills are built

- `skills/{skill}/` is a skill, in the
  [Agent Skills](https://agentskills.io) format: `origin` for the CLI,
  `origincloud-terraform` and `origincloud-sdk`.
- `skills/{skill}/SKILL.md` is written by hand, apart from three parts the
  generator writes on every generation: its frontmatter's `metadata` (the
  versions the skill describes), the block between
  `<!-- BEGIN GENERATED rules -->` and its end (`shared/rules.md`), and the
  block `references` (the map of the references). Edit around them.
- `skills/{skill}/references/{service}.md` is generated, one per service of
  the API. `references/{service}-workflows.md` is written by hand, and the
  map of the references links it.
- `skills/{skill}/evals/` holds the skill's evals (see below).
- `shared/rules.md` holds the rules every surface shares. It is copied into
  every skill's `SKILL.md`, since an installed skill must stand on its own,
  and origincloudhq/mcp serves it as its server's instructions.

The generator, `generator/`, reads the API definitions in
[origincloudhq/api](https://github.com/origincloudhq/api) at the commit
`API_REVISION` names. It downloads the API's dependencies from the Buf
Schema Registry at the commits its `buf.lock` pins, checks them against the
digests it pins, and compiles the definitions with
[protox](https://docs.rs/protox), as origincloudhq/mcp's generator does. For
each service, the packages `origincloud.{service}.{version}`, it builds one
model from the newest version: the resources (`google.api.resource`) with
their patterns, full names and fields, each field's
`google.api.field_behavior`; the methods, which of them answer with a
long-running operation (`google.longrunning.operation_info`), which delete,
and which take a `request_id`; and the definitions' comments.

What the definitions don't say comes from origincloudhq/mcp's
`generator/tools.toml`, at the commit `clients.toml` pins: its `[[one_way]]`
list is the one list of changes that can't be undone, such as growing a
volume, and its `[[no_tool]]` list says which methods an agent isn't handed,
and why. The skills never keep a copy of either.

Each skill's references render the same model in its client's names, taken
from the client release `clients.toml` pins, never derived on their own:

- `origin`: the commands and flags of the CLI release's command tree, the
  `commands.json` it publishes, which maps each command to the API method it
  calls;
- `origincloud-terraform`: the resources, data sources and arguments of the
  provider release's schema, as `tofu providers schema -json` (or
  `terraform`) prints it;
- `origincloud-sdk`: the packages, clients and methods of each SDK by its
  generator's naming rules (`generator/src/sdk.rs`), each method checked
  against the release's source.

Generation fails when a client release generated from the pinned API
revision lacks a name the references would use, and when a skill's files,
the hand-written ones too, use an `origin` command or flag or a Terraform
type the pinned release doesn't have. A client release from an older API
revision gives its references a list of what it doesn't have yet instead.
Each reference links the guide of its service on docs.origincloud.eu, in its
Markdown variant (`…/index.html.md`); `generator/docs.toml` maps services to
guides.

So a change to what a reference says starts in the API or a client, and
reaches the skills through a regeneration; a change to how every reference
says it is a change to the generator. Never edit the generated files.

The regeneration workflow (`.github/workflows/regenerate.yml`) runs
`scripts/update.sh` daily: it moves `clients.toml`'s origincloudhq/mcp
commit to the head of its main and `API_REVISION` to the API revision that
commit was generated from, and the CLI, the provider and the SDKs to their
newest releases, generates again, and opens a pull request when that changes
anything. Once its CI passes, `merge-regeneration.yml` merges it, unless
someone added commits to it, which then wait for review, and
`downstream.yml` dispatches the CLI's regeneration, which embeds the skills.
The merge and the dispatch use a GitHub App's token (the variable
`REGENERATE_APP_ID` and the secret `REGENERATE_APP_PRIVATE_KEY`); without
it, the pull request is merged by hand.

### A new service

When the API gains a service, regeneration writes its references in every
skill. The regeneration pull request names the service, and
`merge-regeneration.yml` leaves it for review rather than merging it: read
its references, and give the service a guide in `generator/docs.toml`.

Nothing else names a service: the generator, the scripts and the coverage
check find every service in the API.

## Evals

`skills/{skill}/evals/evals.json` holds tasks for coding agents: a prompt,
the files the task starts from, and checks of what the agent did, such as
`terraform validate` passing on its configuration, or its transcript
showing that it asked before a replacement rather than applying it.
`scripts/eval.sh` runs a skill's tasks with Claude Code or Codex, with the
skill installed or without it, in a home directory of its own and without
Origin Cloud credentials:

```sh
scripts/eval.sh --skill origincloud-terraform --agent claude
scripts/eval.sh --skill origincloud-terraform --agent claude --without
```

The agent's own credentials come from the environment, since its home is
a new one: `ANTHROPIC_API_KEY`, `CLAUDE_CODE_OAUTH_TOKEN` or a
`CLAUDE_CONFIG_DIR` logged in for the evals for Claude Code, and
`CODEX_API_KEY` or such a `CODEX_HOME` for Codex.

The `origin` skill's tasks run the real `origin` CLI against a fake API,
`evals/fake-api`: a world of resources (`skills/origin/evals/worlds/`)
that it reads, lists, creates, changes and deletes as the API does, with
operations that take a moment, and a log of every request it received. A
task's checks can then ask what the agent sent (`requests_include`,
`requests_exclude`), such as no `DELETE` before the user said yes, or an
attachment only after the volume's operation was done. Point
`EVAL_ORIGIN` at the CLI release the skill describes.

A change to a skill's hand-written parts says in its pull request how the
evals came out, with and without the skill.

## Checks

Rust (the version `rust-toolchain.toml` names), git, curl, OpenTofu or
Terraform, and network access are all you need; the GitHub CLI for the
specification check.

```sh
scripts/generate.sh            # generate the references and SKILL.md parts again
scripts/generate.sh --check    # check they are current, and every service has a reference in every skill
scripts/update.sh              # move to the newest API and client releases, and generate
gh skill publish --dry-run     # check every skill against the Agent Skills specification
cargo fmt --check
cargo clippy --workspace --all-targets -- -D warnings
cargo test --workspace
```

The generator also checks every `SKILL.md`'s frontmatter against the
specification. CI also checks the scripts with shellcheck.

## License

By contributing, you agree that your contribution is licensed under the
Apache License 2.0, like the rest of this repository.
