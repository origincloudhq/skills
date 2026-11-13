//! Checks the names a skill's files use, generated or written by hand,
//! against the pinned client releases: every `origin` command and flag in
//! the `origin` skill's code against the CLI's command tree, and every
//! `origincloud_*` type in the Terraform skill's code against the
//! provider's schema. A rename in a client then fails the next generation
//! instead of leaving a wrong skill.

use std::sync::LazyLock;

use regex::Regex;

use crate::cli::Tree;
use crate::terraform::Provider;

static FENCED: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?ms)^\s*```[^\n]*\n(.*?)^\s*```").expect("a valid pattern"));
static INLINE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"`([^`\n]+)`").expect("a valid pattern"));
static COMMAND: LazyLock<Regex> = LazyLock::new(|| {
    Regex::new(r"(?:^|[\s(|;&$])origin((?:\s+-{0,2}[A-Za-z0-9][A-Za-z0-9-]*)*)")
        .expect("a valid pattern")
});
static TERRAFORM_TYPE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"\borigincloud_[a-z0-9_]*[a-z0-9]").expect("a valid pattern"));

/// The code in a Markdown text: fenced blocks, line by line, and inline
/// code spans outside them.
fn code(text: &str) -> Vec<String> {
    let mut out = Vec::new();
    let mut rest = text.to_owned();
    for block in FENCED.captures_iter(text) {
        // A command continued on the next line with `\` is one line.
        let joined = block[1].replace("\\\n", " ");
        out.extend(joined.lines().map(str::to_owned));
        rest = rest.replace(&block[0], "\n");
    }
    out.extend(INLINE.captures_iter(&rest).map(|c| c[1].to_owned()));
    out
}

/// The `origin` commands and flags in `text` the CLI doesn't have, each as
/// a message naming `file`.
pub fn cli(file: &str, text: &str, tree: &Tree) -> Vec<String> {
    let mut problems = Vec::new();
    for line in code(text) {
        for found in COMMAND.captures_iter(&line) {
            let tokens: Vec<&str> = found[1].split_whitespace().collect();
            let words: Vec<&str> = tokens
                .iter()
                .take_while(|t| !t.starts_with('-'))
                .copied()
                .collect();
            // The longest prefix of the words that is a command or a group.
            let mut path: Vec<&str> = Vec::new();
            for w in &words {
                let mut next = path.clone();
                next.push(w);
                if tree.has_command(&next) {
                    path = next;
                } else {
                    break;
                }
            }
            if path.is_empty() {
                if let Some(first) = words.first() {
                    if !matches!(*first, "help") {
                        problems.push(format!(
                            "{file}: `origin {first}` isn't a command of origin {}",
                            tree.cli_version
                        ));
                    }
                }
                if !words.is_empty() {
                    continue;
                }
            }
            // Flags are checked for a whole command; a group (`origin pvs`)
            // or the bare `origin` only takes the global ones.
            let command = tree.command(&path);
            if command.is_none() && path.len() < words.len() {
                problems.push(format!(
                    "{file}: `origin {}` isn't a command of origin {}",
                    words[..=path.len()].join(" "),
                    tree.cli_version
                ));
                continue;
            }
            for t in tokens.iter().filter(|t| t.starts_with('-')) {
                let flag = t.trim_start_matches('-');
                let flag = flag.split('=').next().unwrap_or(flag);
                if matches!(flag, "help" | "h" | "version" | "V" | "") {
                    continue;
                }
                let known = match command {
                    Some(c) => tree.has_flag(c, flag),
                    None => tree.global_args.iter().any(|a| {
                        a.long.as_deref() == Some(flag) || a.short.as_deref() == Some(flag)
                    }),
                };
                if !known {
                    problems.push(format!(
                        "{file}: `origin {}` has no flag `{t}` in origin {}",
                        path.join(" "),
                        tree.cli_version
                    ));
                }
            }
        }
    }
    problems
}

/// The `origincloud_*` types in `text` the provider doesn't have.
pub fn terraform(file: &str, text: &str, provider: &Provider, version: &str) -> Vec<String> {
    let mut problems = Vec::new();
    for line in code(text) {
        for found in TERRAFORM_TYPE.find_iter(&line) {
            let name = found.as_str();
            if !provider.resource_schemas.contains_key(name)
                && !provider.data_source_schemas.contains_key(name)
            {
                problems.push(format!(
                    "{file}: `{name}` isn't a resource or data source of the provider {version}"
                ));
            }
        }
    }
    problems.sort();
    problems.dedup();
    problems
}

#[cfg(test)]
mod tests {
    use super::*;

    fn tree() -> Tree {
        serde_json::from_str(
            r#"{"cli_version":"1.0.0","api_revision":null,
                "commands":[{"path":["pvs","create-volume"],"method":"x","args":[{"long":"size-gib"},{"long":"no-wait"}]},
                            {"path":["id","login"],"method":null,"args":[{"long":"device"}]}],
                "global_args":[{"long":"output","short":"o"}]}"#,
        )
        .unwrap()
    }

    #[test]
    fn known_commands_and_flags_pass() {
        let text = "Run `origin pvs create-volume --size-gib 10 -o json`, or\n\n```sh\norigin id login \\\n  --device\norigin pvs <command>\n```\n";
        assert_eq!(cli("a.md", text, &tree()), Vec::<String>::new());
    }

    #[test]
    fn unknown_commands_and_flags_fail() {
        let text = "`origin pvs create-volumes`, `origin pvs create-volume --size 1`, `origin storage list`";
        let problems = cli("a.md", text, &tree());
        assert_eq!(problems.len(), 3, "{problems:?}");
        // Prose isn't code.
        assert!(cli("a.md", "the origin storage of it", &tree()).is_empty());
    }
}
