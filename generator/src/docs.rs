//! Descriptions from the proto comments: without the API linter's
//! directives, `(-- ... --)`, and without the blank lines around them.

use std::collections::HashMap;
use std::sync::LazyLock;

use prost_types::FileDescriptorSet;
use regex::Regex;

static LINTER_DIRECTIVE: LazyLock<Regex> =
    LazyLock::new(|| Regex::new(r"(?s)\(--.*?--\)").expect("a valid pattern"));

/// The leading comments of the API's files, by file and location path.
pub struct Comments {
    leading: HashMap<(String, Vec<i32>), String>,
}

impl Comments {
    pub fn new(fds: &FileDescriptorSet) -> Self {
        let mut leading = HashMap::new();
        for file in &fds.file {
            let Some(info) = &file.source_code_info else {
                continue;
            };
            for location in &info.location {
                if let Some(text) = &location.leading_comments {
                    leading.insert(
                        (file.name().to_owned(), location.path.clone()),
                        text.clone(),
                    );
                }
            }
        }
        Self { leading }
    }

    /// The leading comment at `path` in `file`; empty when there is none.
    pub fn leading(&self, file: &str, path: &[i32]) -> &str {
        self.leading
            .get(&(file.to_owned(), path.to_vec()))
            .map_or("", String::as_str)
    }
}

/// `comment` without linter directives, its lines unindented and joined
/// into paragraphs, the paragraphs separated by a blank line.
pub fn clean(comment: &str) -> String {
    let text = LINTER_DIRECTIVE.replace_all(comment, "");
    let mut paragraphs: Vec<String> = Vec::new();
    let mut current: Vec<&str> = Vec::new();
    for line in text.lines() {
        let line = line.trim();
        if line.is_empty() {
            if !current.is_empty() {
                paragraphs.push(current.join(" "));
                current.clear();
            }
        } else {
            current.push(line);
        }
    }
    if !current.is_empty() {
        paragraphs.push(current.join(" "));
    }
    paragraphs.join("\n\n")
}

/// The first sentence of `text`'s first paragraph.
pub fn first_sentence(text: &str) -> &str {
    let paragraph = text.split("\n\n").next().unwrap_or_default();
    let mut end = paragraph.len();
    let bytes = paragraph.as_bytes();
    for (i, b) in bytes.iter().enumerate() {
        if *b == b'.' && (i + 1 == bytes.len() || bytes[i + 1] == b' ') {
            // Not the dot of "e.g." or of a name in backticks.
            let before = &paragraph[..i];
            if before.matches('`').count().is_multiple_of(2)
                && !before.ends_with("e.g")
                && !before.ends_with("i.e")
            {
                end = i + 1;
                break;
            }
        }
    }
    &paragraph[..end]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn directives_go_and_lines_join() {
        assert_eq!(
            clean(
                " Deletes a volume. It must not\n be attached.\n\n (-- api-linter: x\n    y --)\n"
            ),
            "Deletes a volume. It must not be attached."
        );
        assert_eq!(clean(" One.\n\n Two.\n"), "One.\n\nTwo.");
    }

    #[test]
    fn the_first_sentence_ends_at_a_dot_outside_backticks() {
        assert_eq!(first_sentence("Sets `a.b`. Then more."), "Sets `a.b`.");
        assert_eq!(first_sentence("No dot"), "No dot");
    }
}
