//! The provider's schema, as `terraform providers schema -json` (or
//! `tofu providers schema -json`) prints it for the pinned release.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
struct Document {
    provider_schemas: BTreeMap<String, Provider>,
}

#[derive(Debug, Deserialize)]
pub struct Provider {
    #[serde(default)]
    pub resource_schemas: BTreeMap<String, Schema>,
    #[serde(default)]
    pub data_source_schemas: BTreeMap<String, Schema>,
}

#[derive(Debug, Deserialize)]
pub struct Schema {
    pub block: Block,
}

#[derive(Debug, Deserialize)]
pub struct Block {
    #[serde(default)]
    pub attributes: BTreeMap<String, Attribute>,
    #[serde(default)]
    pub block_types: BTreeMap<String, NestedBlock>,
    #[serde(default)]
    pub description: String,
}

#[derive(Debug, Deserialize)]
pub struct Attribute {
    #[serde(default)]
    pub description: String,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub optional: bool,
    #[serde(default)]
    pub computed: bool,
    #[serde(default)]
    pub sensitive: bool,
}

#[derive(Debug, Deserialize)]
pub struct NestedBlock {
    pub nesting_mode: String,
    pub block: Block,
}

/// The provider's schema, whichever registry address the tool keyed it
/// under (`registry.terraform.io/...` or `registry.opentofu.org/...`).
pub fn load(path: &Path, source: &str) -> Result<Provider> {
    let text =
        std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
    let document: Document =
        serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))?;
    document
        .provider_schemas
        .into_iter()
        .find(|(address, _)| address.ends_with(&format!("/{source}")))
        .map(|(_, p)| p)
        .with_context(|| format!("{} has no schema of {source}", path.display()))
}

impl Attribute {
    /// `required`, `optional`, `optional, computed` or `computed`.
    pub fn mode(&self) -> &'static str {
        match (self.required, self.optional, self.computed) {
            (true, _, _) => "required",
            (_, true, true) => "optional, computed",
            (_, true, false) => "optional",
            _ => "computed",
        }
    }

    /// Whether changing it replaces the resource, as its description says.
    pub fn replaces(&self) -> bool {
        self.description.contains("replaces the")
    }
}
