//! What origincloudhq/mcp's `generator/tools.toml` says about methods that
//! the API definitions don't: the changes that can't be undone
//! (`[[one_way]]`), and the methods an agent isn't handed a tool for
//! (`[[no_tool]]`), each with its reason. That table is the one list of
//! one-way changes: the skills read it from the pinned commit of
//! origincloudhq/mcp rather than keeping a copy.

use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// The parts of `tools.toml` the skills use; its other tables are the MCP
/// tools' own business.
#[derive(Debug, Default, Deserialize)]
pub struct Policy {
    #[serde(default)]
    pub no_tool: Vec<NoTool>,
    #[serde(default)]
    pub one_way: Vec<OneWay>,
}

#[derive(Debug, Deserialize)]
pub struct NoTool {
    pub service: String,
    pub method: String,
    pub why: String,
}

#[derive(Debug, Deserialize)]
pub struct OneWay {
    pub service: String,
    pub method: String,
    /// A path from the request, such as `volume.size_gib`.
    pub field: String,
    /// What can't be undone.
    pub consequence: String,
}

impl Policy {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reads_the_mcp_table_and_ignores_the_rest() {
        let policy: Policy = toml::from_str(
            r#"
            [[exclude]]
            service = "pvs"
            method = "ImportVolume"
            field = "volume.source_snapshot"
            why = "y"

            [[one_way]]
            service = "pvs"
            method = "UpdateVolume"
            field = "volume.size_gib"
            consequence = "A volume never shrinks back."
            "#,
        )
        .unwrap();
        assert_eq!(policy.one_way.len(), 1);
        assert_eq!(
            policy.one_way[0].consequence,
            "A volume never shrinks back."
        );
        assert!(policy.no_tool.is_empty());
    }
}
