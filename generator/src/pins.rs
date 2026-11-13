//! What the skills are generated from besides the API (`API_REVISION`):
//! the client releases they describe, in `clients.toml`, and the docs page
//! of each service, in `generator/docs.toml`.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

/// `clients.toml`.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Clients {
    #[allow(dead_code)]
    pub mcp: Mcp,
    pub cli: Cli,
    pub terraform: Terraform,
    pub sdk: BTreeMap<String, String>,
}

/// Read by scripts/generate.sh, which fetches the table.
#[allow(dead_code)]
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Mcp {
    /// The commit of origincloudhq/mcp whose `generator/tools.toml` lists
    /// the one-way changes.
    pub revision: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Cli {
    pub version: String,
    /// `release` for the `commands.json` of the release, or a file of this
    /// repository. Read by scripts/generate.sh, which fetches it.
    #[allow(dead_code)]
    pub commands: String,
}

#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Terraform {
    /// `origincloudhq/origincloud`.
    pub source: String,
    pub version: String,
}

impl Clients {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
    }
}

/// `generator/docs.toml`: the guide on docs.origincloud.eu each service's
/// references link to.
#[derive(Debug, Deserialize)]
#[serde(deny_unknown_fields)]
pub struct Docs {
    /// `https://docs.origincloud.eu/docs`.
    pub base: String,
    /// The guide's path under `base`, by service: `storage` for `pvs`.
    pub guides: BTreeMap<String, String>,
}

impl Docs {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        toml::from_str(&text).with_context(|| format!("reading {}", path.display()))
    }

    /// The Markdown variant of the service's guide: `…/storage/index.html.md`;
    /// none for a service without a guide.
    pub fn markdown(&self, service: &str) -> Option<String> {
        self.guides.get(service).map(|g| {
            format!(
                "{}/{}/index.html.md",
                self.base.trim_end_matches('/'),
                g.trim_matches('/')
            )
        })
    }
}
