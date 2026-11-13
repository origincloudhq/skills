//! A skill's `SKILL.md`: written by hand, but for the parts the generator
//! keeps current in it, which it writes again on every generation:
//!
//! - the frontmatter's `metadata`, the versions the skill describes;
//! - each block between `<!-- BEGIN GENERATED {name} -->` and
//!   `<!-- END GENERATED {name} -->`: `rules`, the shared rules
//!   (`shared/rules.md`), and `references`, the map of the references.
//!
//! It also checks the frontmatter against the Agent Skills specification.

use std::collections::BTreeMap;

use anyhow::{bail, ensure, Context, Result};

const FENCE: &str = "---";

/// `text` with its generated parts replaced: the frontmatter's `metadata`
/// by `metadata`, and each block named in `blocks` by its content.
pub fn splice(
    text: &str,
    metadata: &BTreeMap<String, String>,
    blocks: &BTreeMap<&str, String>,
) -> Result<String> {
    let (frontmatter, body) = split(text)?;
    let mut kept: Vec<&str> = Vec::new();
    let mut in_metadata = false;
    for line in frontmatter.lines() {
        if line.starts_with("metadata:") {
            in_metadata = true;
            continue;
        }
        if in_metadata && (line.starts_with(' ') || line.is_empty()) {
            continue;
        }
        in_metadata = false;
        kept.push(line);
    }
    let mut out = format!("{FENCE}\n");
    for line in kept {
        out.push_str(line);
        out.push('\n');
    }
    if !metadata.is_empty() {
        out.push_str("metadata:\n");
        for (key, value) in metadata {
            out.push_str(&format!("  {key}: \"{value}\"\n"));
        }
    }
    out.push_str(FENCE);
    out.push('\n');

    let mut body = body.to_owned();
    for (name, content) in blocks {
        let begin = format!("<!-- BEGIN GENERATED {name} -->");
        let end = format!("<!-- END GENERATED {name} -->");
        let start = body
            .find(&begin)
            .with_context(|| format!("SKILL.md has no {begin}"))?;
        let stop = body
            .find(&end)
            .with_context(|| format!("SKILL.md has no {end}"))?;
        ensure!(start < stop, "SKILL.md has {end} before {begin}");
        let content = content.trim_end();
        body = format!("{}{begin}\n{content}\n{}", &body[..start], &body[stop..]);
    }
    out.push_str(&body);
    Ok(out)
}

/// The frontmatter, without its fences, and the body after it.
fn split(text: &str) -> Result<(&str, &str)> {
    let rest = text
        .strip_prefix("---\n")
        .context("SKILL.md must start with YAML frontmatter, `---`")?;
    let end = rest
        .find("\n---\n")
        .context("SKILL.md's frontmatter has no closing `---`")?;
    Ok((&rest[..end + 1], &rest[end + 5..]))
}

/// The frontmatter's top-level keys and their values; a folded or literal
/// block value (`>-`, `|`) is joined into one line.
fn keys(frontmatter: &str) -> Result<BTreeMap<String, String>> {
    let mut out: BTreeMap<String, String> = BTreeMap::new();
    let mut current: Option<String> = None;
    for line in frontmatter.lines() {
        if line.trim().is_empty() {
            continue;
        }
        if line.starts_with(' ') {
            let key = current
                .as_ref()
                .context("an indented line before any key")?;
            let value = out.entry(key.clone()).or_default();
            if !value.is_empty() {
                value.push(' ');
            }
            value.push_str(line.trim());
            continue;
        }
        let (key, value) = line
            .split_once(':')
            .with_context(|| format!("frontmatter line {line:?} isn't `key: value`"))?;
        let value = value.trim();
        let value = if matches!(value, ">" | ">-" | "|" | "|-") {
            String::new()
        } else {
            value.trim_matches('"').to_owned()
        };
        ensure!(
            out.insert(key.to_owned(), value).is_none(),
            "frontmatter key {key} is given twice"
        );
        current = Some(key.to_owned());
    }
    Ok(out)
}

/// Checks a `SKILL.md` against the Agent Skills specification: `name` and
/// `description` are given, the name is lower-case words joined by hyphens,
/// at most 64 characters and the skill's directory, the description at most
/// 1024 characters, and no key is one the specification doesn't have.
pub fn validate(directory: &str, text: &str) -> Result<()> {
    let (frontmatter, body) = split(text)?;
    let keys = keys(frontmatter)?;
    for key in keys.keys() {
        if !matches!(
            key.as_str(),
            "name" | "description" | "license" | "compatibility" | "metadata" | "allowed-tools"
        ) {
            bail!("{directory}/SKILL.md: the frontmatter key {key} isn't in the Agent Skills specification");
        }
    }
    let name = keys.get("name").map(String::as_str).unwrap_or_default();
    let valid = !name.is_empty()
        && name.len() <= 64
        && name.split('-').all(|w| {
            !w.is_empty()
                && w.bytes()
                    .all(|b| b.is_ascii_lowercase() || b.is_ascii_digit())
        });
    ensure!(valid, "{directory}/SKILL.md: name {name:?} isn't lower-case words joined by single hyphens, at most 64 characters");
    ensure!(
        name == directory,
        "{directory}/SKILL.md: name {name:?} isn't its directory's"
    );
    let description = keys
        .get("description")
        .map(String::as_str)
        .unwrap_or_default();
    ensure!(
        !description.is_empty() && description.chars().count() <= 1024,
        "{directory}/SKILL.md: the description must be 1 to 1024 characters, not {}",
        description.chars().count()
    );
    if let Some(c) = keys.get("compatibility") {
        ensure!(
            c.chars().count() <= 500,
            "{directory}/SKILL.md: compatibility is over 500 characters"
        );
    }
    ensure!(!body.trim().is_empty(), "{directory}/SKILL.md has no body");
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    const SKILL: &str = "---\nname: demo\ndescription: >-\n  Does things\n  well.\nmetadata:\n  old: \"1\"\n---\n# Demo\n\n<!-- BEGIN GENERATED rules -->\nstale\n<!-- END GENERATED rules -->\n\nHand-written.\n";

    #[test]
    fn splices_the_generated_parts_and_keeps_the_rest() {
        let metadata = BTreeMap::from([("cli-version".to_owned(), "1.18.0".to_owned())]);
        let blocks = BTreeMap::from([("rules", "Rule one.\n".to_owned())]);
        let out = splice(SKILL, &metadata, &blocks).unwrap();
        assert_eq!(
            out,
            "---\nname: demo\ndescription: >-\n  Does things\n  well.\nmetadata:\n  cli-version: \"1.18.0\"\n---\n# Demo\n\n<!-- BEGIN GENERATED rules -->\nRule one.\n<!-- END GENERATED rules -->\n\nHand-written.\n"
        );
        // Splicing again changes nothing.
        assert_eq!(splice(&out, &metadata, &blocks).unwrap(), out);
        validate("demo", &out).unwrap();
    }

    #[test]
    fn a_missing_block_fails() {
        let blocks = BTreeMap::from([("references", String::new())]);
        assert!(splice(SKILL, &BTreeMap::new(), &blocks).is_err());
    }

    #[test]
    fn invalid_frontmatter_fails() {
        assert!(validate("demo", "---\nname: Demo\ndescription: x\n---\nbody\n").is_err());
        assert!(validate("demo", "---\nname: other\ndescription: x\n---\nbody\n").is_err());
        assert!(validate("demo", "---\nname: demo\n---\nbody\n").is_err());
        assert!(validate(
            "demo",
            "---\nname: demo\ndescription: x\nversion: 1\n---\nbody\n"
        )
        .is_err());
        assert!(validate("demo", "---\nname: demo--x\ndescription: x\n---\nbody\n").is_err());
        validate("demo", "---\nname: demo\ndescription: x\n---\nbody\n").unwrap();
    }
}
