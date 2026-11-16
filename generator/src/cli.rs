//! The CLI's command tree, as a CLI release publishes it (`commands.json`):
//! every command's path, flags and the API method it calls; and the methods
//! the release leaves without a command, with the reason, from the
//! `[[no_command]]` list of its `generator/commands.toml` at its tag.

use std::collections::BTreeMap;
use std::path::Path;

use anyhow::{Context, Result};
use serde::Deserialize;

#[derive(Debug, Deserialize)]
pub struct Tree {
    pub cli_version: String,
    /// The API commit the release's generated commands come from.
    pub api_revision: Option<String>,
    pub commands: Vec<Command>,
    #[serde(default)]
    pub global_args: Vec<Arg>,
    /// The reason the release has no command for a method, by the method's
    /// service (`billing`) and name (`DownloadLegalDocumentVersion`).
    #[serde(skip)]
    pub no_command: BTreeMap<(String, String), String>,
}

/// The parts of the CLI's `generator/commands.toml` the skills read.
#[derive(Debug, Default, Deserialize)]
struct Table {
    #[serde(default)]
    no_command: Vec<NoCommand>,
}

#[derive(Debug, Deserialize)]
struct NoCommand {
    service: String,
    method: String,
    why: String,
}

// The format's other fields (`about`, `long_running`, a flag's `help` and
// `possible_values`) aren't rendered: the references take descriptions
// from the API, so that every skill says the same.
#[derive(Debug, Deserialize)]
pub struct Command {
    /// `["pvs", "create-volume"]`.
    pub path: Vec<String>,
    /// `origincloud.pvs.v1beta1.VolumeService.CreateVolume`; none for a
    /// hand-written command.
    pub method: Option<String>,
    #[serde(default)]
    pub args: Vec<Arg>,
}

#[derive(Debug, Deserialize)]
pub struct Arg {
    pub long: Option<String>,
    #[serde(default)]
    pub short: Option<String>,
    #[serde(default)]
    pub value_name: Option<String>,
    #[serde(default)]
    pub required: bool,
    #[serde(default)]
    pub multiple: bool,
    #[serde(default)]
    pub global: bool,
}

impl Tree {
    pub fn load(path: &Path) -> Result<Self> {
        let text =
            std::fs::read_to_string(path).with_context(|| format!("reading {}", path.display()))?;
        serde_json::from_str(&text).with_context(|| format!("reading {}", path.display()))
    }

    /// Takes the release's `[[no_command]]` list from its
    /// `generator/commands.toml`, `table`.
    pub fn with_table(mut self, table: &str) -> Result<Self> {
        let table: Table = toml::from_str(table).context("reading the CLI's commands.toml")?;
        self.no_command = table
            .no_command
            .into_iter()
            .map(|n| ((n.service, n.method), n.why))
            .collect();
        Ok(self)
    }

    /// Why the release has no command for `method` of `service`, when its
    /// table says.
    pub fn no_command(&self, service: &str, method: &str) -> Option<&str> {
        self.no_command
            .get(&(service.to_owned(), method.to_owned()))
            .map(String::as_str)
    }

    /// The command calling each method, by the method's full name. The
    /// operation methods are commands of every service; they are keyed
    /// `{service}:{method}` instead.
    pub fn by_method(&self) -> BTreeMap<String, &Command> {
        let mut out = BTreeMap::new();
        for c in &self.commands {
            if let Some(m) = &c.method {
                let key = if m.starts_with("origincloud.longrunning.") {
                    format!("{}:{}", c.path[0], m.rsplit('.').next().unwrap_or_default())
                } else {
                    m.clone()
                };
                out.insert(key, c);
            }
        }
        out
    }

    /// Whether `origin {words}` is a command, or a group of commands.
    pub fn has_command(&self, words: &[&str]) -> bool {
        self.commands
            .iter()
            .any(|c| c.path.len() >= words.len() && c.path.iter().zip(words).all(|(a, b)| a == b))
    }

    /// The command `origin {words}` names exactly.
    pub fn command(&self, words: &[&str]) -> Option<&Command> {
        self.commands
            .iter()
            .find(|c| c.path.len() == words.len() && c.path.iter().zip(words).all(|(a, b)| a == b))
    }

    /// Whether `--{flag}` is a flag of `command` or a global one.
    pub fn has_flag(&self, command: &Command, flag: &str) -> bool {
        let named = |a: &Arg| a.long.as_deref() == Some(flag) || a.short.as_deref() == Some(flag);
        command.args.iter().any(named) || self.global_args.iter().any(named)
    }
}

impl Arg {
    /// `--size-gib <SIZE_GIB>`.
    pub fn usage(&self) -> String {
        let mut s = match (&self.long, &self.short) {
            (Some(l), _) => format!("--{l}"),
            (None, Some(s)) => format!("-{s}"),
            (None, None) => self.value_name.clone().unwrap_or_default(),
        };
        if let Some(v) = &self.value_name {
            if self.long.is_some() || self.short.is_some() {
                s.push_str(&format!(" <{v}>"));
            }
        }
        if self.multiple {
            s.push_str(" ...");
        }
        s
    }
}
