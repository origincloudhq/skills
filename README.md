# Origin Cloud agent skills

Agent skills that teach coding agents (Claude Code, Codex, Cursor, Gemini
CLI, GitHub Copilot and any other agent that reads the open
[Agent Skills](https://agentskills.io) format) to use
[Origin Cloud](https://origincloud.eu) correctly:

| Skill | For |
|---|---|
| [`origin`](skills/origin/SKILL.md) | The `origin` command-line interface |
| [`origincloud-terraform`](skills/origincloud-terraform/SKILL.md) | Terraform and OpenTofu with the `origincloudhq/origincloud` provider |
| [`origincloud-sdk`](skills/origincloud-sdk/SKILL.md) | The SDKs for Rust, Go and TypeScript |

Each skill is a short hand-written `SKILL.md` and a reference per service of
the API, `references/{service}.md`, generated from the API definitions in
[origincloudhq/api](https://github.com/origincloudhq/api) and the client
release the skill describes: its commands, Terraform types or SDK methods,
which changes to ask about first, which to wait for, and the API's resources
with their full names and fields. Every skill carries the same rules
([shared/rules.md](shared/rules.md)): use the login the user has and never
print a credential, read before changing, ask before deletes and changes
that can't be undone, wait on operations, name resources by their full
names, and report errors with their code and correlation ID. The
[MCP tools](https://github.com/origincloudhq/mcp) give an agent the same
rules as their server's instructions.

## Install

With the GitHub CLI:

```sh
gh skill install origincloudhq/skills origin
gh skill install origincloudhq/skills origincloud-terraform
gh skill install origincloudhq/skills origincloud-sdk
```

Or copy a skill's directory into your agent's skills directory, such as
`.claude/skills/` for Claude Code or `.agents/skills/` for Codex.

Each skill names the release it describes in its frontmatter's `metadata`
(`cli-version`, `provider-version`, `sdk-*-version`): use it with that
release or a later one.

## Contributing

See [CONTRIBUTING.md](CONTRIBUTING.md), which describes how the references
are generated.

## License

Apache License 2.0; see [LICENSE](LICENSE).
